// A client example

use std::{error::Error, path::Path};

use chrono::Utc;
use nix::sys::utsname::uname;
use presenced::{Message, socket_decode, socket_encode};
use tokio::{fs::File, io::AsyncReadExt, net::UnixStream};
use zbus::{Connection, Proxy};

async fn get_desktop_from_systemd() -> Option<String> {
    let connection = Connection::session().await.ok()?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await
    .ok()?;
    let env: Vec<String> = proxy.get_property("Environment").await.ok()?;
    env.iter()
        .find(|s| s.starts_with("XDG_CURRENT_DESKTOP="))
        .and_then(|s| s.split('=').nth(1).map(|s| s.to_string()))
}

#[tokio::main]
async fn main() {
    let xdg_runtime_dir = dirs::runtime_dir().unwrap_or("/tmp".into());
    let socket_path = xdg_runtime_dir.join("discord-ipc-0");
    let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);
    loop {
        if let Err(e) = handle_connect(&socket_path).await {
            eprintln!("Error: {:?}", e);
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn handle_connect(socket_path: &Path) -> Result<(), Box<dyn Error>> {
    let kernel_version = uname()?.release().to_str().unwrap_or("unknown").to_owned();
    let uptime: u64 = {
        let mut f = File::open("/proc/uptime").await?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).await?;
        let seconds: f64 = buf
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned()
            .parse()
            .unwrap_or(0.0);
        let current_time = Utc::now().timestamp() as u64;
        current_time - seconds as u64
    };
    let distro = {
        let mut f = File::open("/etc/os-release").await?;
        let mut buf = String::new();
        f.read_to_string(&mut buf).await?;
        let buf = buf
            .lines()
            .find(|line| line.starts_with("PRETTY_NAME="))
            .map(|line| line.split('=').nth(1).unwrap_or("unknown").to_owned())
            .unwrap_or("unknown".to_owned());
        buf.trim_matches('"').to_owned()
    };
    // Try getting the desktop environment (wait for 1 minute)
    let mut retries = 60;
    let desktop = loop {
        if let Some(desktop) = get_desktop_from_systemd().await {
            break desktop;
        }
        if let Some(desktop) = std::env::var("XDG_CURRENT_DESKTOP").ok() {
            break desktop;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        retries -= 1;
        if retries == 0 {
            break "unknown".to_owned();
        }
    };
    let mut stream = UnixStream::connect(&socket_path).await?;
    println!("Connected to Discord presence IPC");
    socket_encode(
        &mut stream,
        Message {
            opcode: 0,
            payload: serde_json::json!({
                "client_id": "1302583306281418823",
            }),
        },
    )
    .await?;
    socket_decode(&mut stream).await?;
    socket_encode(
        &mut stream,
        Message {
            opcode: 1,
            payload: serde_json::json!({
                "cmd": "SET_ACTIVITY",
                "args": {
                    "activity": {
                        "state": kernel_version,
                        "details": format!("{} ({})", distro, desktop),
                        "timestamps": {
                            "start": uptime,
                        }
                    }
                }
            }),
        },
    )
    .await?;
    loop {
        let mut buf = [0u8; 1024];
        #[allow(clippy::unused_io_amount)]
        if stream.read(&mut buf).await? == 0 {
            return Ok(());
        }
    }
}
