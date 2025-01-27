use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::Path,
    sync::{Mutex, OnceLock},
};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    task::JoinSet,
};
use tracing::{debug, info, warn};

mod wellknown;
use wellknown::CLIENT_MAPPINGS;

#[derive(Debug)]
struct State {
    large_text: String,
    small_text: String,
    state: String,
    details: String,
    start_time: Option<std::time::SystemTime>,
    end_time: Option<std::time::SystemTime>,
}

// client_id -> state
static STATE: OnceLock<Mutex<HashMap<String, State>>> = OnceLock::new();

#[derive(Debug)]
struct Message {
    opcode: u32,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct Frame {
    cmd: String,
    args: FrameArgs,
}

#[derive(Debug, Deserialize)]
struct FrameArgs {
    #[serde(default)]
    activity: FrameActivity,
}

#[derive(Debug, Deserialize, Default)]
struct FrameActivity {
    #[serde(default)]
    assets: FrameAssets,
    #[serde(default)]
    #[allow(unused)]
    buttons: Vec<FrameButton>,
    #[serde(default)]
    details: String,
    #[serde(default)]
    timestamps: FrameTimestamps,
    state: String,
}

fn number_to_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(n) => {
            if let Some(n) = n.as_u64() {
                Ok(Some(n))
            } else if let Some(n) = n.as_f64() {
                Ok(Some(n as u64))
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize, Default)]
struct FrameTimestamps {
    #[serde(default)]
    #[serde(deserialize_with = "number_to_u64")]
    start: Option<u64>,
    #[serde(default)]
    #[serde(deserialize_with = "number_to_u64")]
    end: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FrameButton {
    #[allow(unused)]
    label: String,
    #[allow(unused)]
    url: String,
}

#[derive(Debug, Deserialize, Default)]
struct FrameAssets {
    #[serde(default)]
    #[allow(unused)]
    large_image: String,
    #[serde(default)]
    large_text: String,
    #[serde(default)]
    #[allow(unused)]
    small_image: String,
    #[serde(default)]
    small_text: String,
}

async fn socket_decode(socket: &mut UnixStream) -> Result<Message, Box<dyn Error>> {
    let mut opcode = [0u8; 4];
    socket.read_exact(&mut opcode).await?;
    let opcode = u32::from_le_bytes(opcode);
    let mut length = [0u8; 4];
    socket.read_exact(&mut length).await?;
    let length = u32::from_le_bytes(length);
    // reject if length is greater than 1M
    if length > 1_000_000 {
        return Err("Payload too large".into());
    }
    let mut payload = vec![0u8; length as usize];
    socket.read_exact(&mut payload).await?;
    let payload = String::from_utf8(payload)?;
    let payload = serde_json::from_str::<serde_json::Value>(&payload)?;
    Ok(Message { opcode, payload })
}

async fn socket_encode(socket: &mut UnixStream, message: Message) -> Result<(), Box<dyn Error>> {
    let opcode = message.opcode.to_le_bytes();
    socket.write_all(&opcode).await?;
    let payload = serde_json::to_string(&message.payload)?;
    let length = payload.len() as u32;
    let length = length.to_le_bytes();
    socket.write_all(&length).await?;
    socket.write_all(payload.as_bytes()).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let xdg_runtime_dir = dirs::runtime_dir().unwrap_or("/tmp".into());
    let paths = [
        xdg_runtime_dir.join("app/com.discordapp.Discord/discord-ipc-0"),
        xdg_runtime_dir.join("discord-ipc-0"),
    ];
    tracing_subscriber::fmt::init();
    let mut join_set = JoinSet::new();
    join_set.spawn(async move {
        periodic_print().await;
    });
    for path in paths.iter() {
        if Path::new(path).exists() {
            fs::remove_file(path)?;
        }

        let listener = UnixListener::bind(path)?;
        info!("Listening on: {}", path.display());

        join_set.spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.expect("accept failed");
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket).await {
                        warn!("Error: {:?}", e);
                    }
                });
            }
        });
    }

    join_set.join_all().await;

    Ok(())
}

fn client_id_to_name(client_id: &str) -> String {
    CLIENT_MAPPINGS.get(client_id).unwrap_or(&client_id).to_string()
}

async fn periodic_print() {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let state = STATE.get().unwrap().lock().unwrap();
        for (client_id, state) in state.iter() {
            info!(
                "client: {}, state:\n  large_text: {}\n  small_text: {}\n  state: {}\n  details: {}\n  start_time: {}\n  end_time: {}",
                client_id_to_name(client_id),
                state.large_text,
                state.small_text,
                state.state,
                state.details,
                match state.start_time {
                    Some(d) => DateTime::<Utc>::from(d).to_rfc3339(),
                    None => "".to_string(),
                },
                match state.end_time {
                    Some(d) => DateTime::<Utc>::from(d).to_rfc3339(),
                    None => "".to_string(),
                },
            );
        }
    }
}

async fn handle_connection(mut socket: UnixStream) -> Result<(), Box<dyn Error>> {
    // Handshake
    let message = socket_decode(&mut socket).await?;
    if message.opcode != 0 {
        return Err("Handshake not received".into());
    }
    let client_id = message.payload["client_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    info!("Handshake with client: {}", client_id_to_name(&client_id));
    let handshake_resp = Message {
        opcode: 1,
        payload: serde_json::json!({"cmd": "DISPATCH", "evt": "READY", "data": {
            "user": {
                "id": "1"
            }
        }}),
    };
    socket_encode(&mut socket, handshake_resp).await?;

    async fn handle_inner(client_id: &str, mut socket: UnixStream) -> Result<(), Box<dyn Error>> {
        loop {
            let message = socket_decode(&mut socket).await?;
            debug!("Message: {:?}", message);
            match message.opcode {
                1 => {
                    if client_id.is_empty() {
                        return Err("Handshake not completed".into());
                    }
                    let message: Frame = serde_json::from_value(message.payload)?;
                    if message.cmd == "SET_ACTIVITY" {
                        let state = State {
                            large_text: message.args.activity.assets.large_text,
                            small_text: message.args.activity.assets.small_text,
                            state: message.args.activity.state,
                            details: message.args.activity.details,
                            start_time: message.args.activity.timestamps.start.map(|timestamps| {
                                if timestamps < 9999999999 {
                                    std::time::UNIX_EPOCH
                                        + std::time::Duration::from_secs(timestamps)
                                } else {
                                    std::time::UNIX_EPOCH
                                        + std::time::Duration::from_millis(timestamps)
                                }
                            }),
                            end_time: message.args.activity.timestamps.end.map(|timestamps| {
                                if timestamps < 9999999999 {
                                    std::time::UNIX_EPOCH
                                        + std::time::Duration::from_secs(timestamps)
                                } else {
                                    std::time::UNIX_EPOCH
                                        + std::time::Duration::from_millis(timestamps)
                                }
                            }),
                        };
                        STATE
                            .get()
                            .unwrap()
                            .lock()
                            .unwrap()
                            .insert(client_id.to_string(), state);
                    } else {
                        warn!("Unknown command: {:?}", message);
                    }
                }
                _ => {
                    warn!("Unknown opcode: {:?}", message);
                }
            }
        }
    }

    let res = handle_inner(&client_id, socket).await;
    STATE.get().unwrap().lock().unwrap().remove(&client_id);

    res
}
