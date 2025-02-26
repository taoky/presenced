use std::{
    collections::HashMap,
    error::Error,
    fs,
    path::Path,
    sync::{Arc, Mutex, OnceLock},
};

use chrono::{DateTime, Local};
use presenced::{socket_decode, socket_encode, Message};
use serde::Deserialize;
use tokio::{
    net::{UnixListener, UnixStream},
    task::JoinSet,
};
use tracing::{debug, info, warn};

mod wellknown;
use wellknown::CLIENT_MAPPINGS;

#[derive(Debug, Clone)]
struct State {
    large_text: String,
    small_text: String,
    state: String,
    details: String,
    start_time: Option<std::time::SystemTime>,
    end_time: Option<std::time::SystemTime>,
}

type SharedState = Arc<Mutex<HashMap<String, State>>>;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::fmt().init();
    EXPECTED_TOKEN.get_or_init(|| std::env::var("TOKEN").expect("TOKEN env var not set"));
    UPSTREAM_URL.get_or_init(|| {
        std::env::var("UPSTREAM").unwrap_or_else(|_| "http://localhost:3001".to_string())
    });
    info!(
        "Starting up, using upstream {}",
        *UPSTREAM_URL.get().unwrap()
    );
    let state: SharedState = Arc::new(Mutex::new(HashMap::new()));
    let xdg_runtime_dir = dirs::runtime_dir().unwrap_or("/tmp".into());
    let paths = [
        xdg_runtime_dir.join("app/com.discordapp.Discord/discord-ipc-0"),
        xdg_runtime_dir.join("discord-ipc-0"),
    ];
    let mut join_set = JoinSet::new();

    let state_1 = state.clone();
    join_set.spawn(async move {
        periodic_send(state_1).await;
    });
    let _ = sd_notify::notify(true, &[sd_notify::NotifyState::Ready]);
    for path in paths.iter() {
        if Path::new(path).exists() {
            fs::remove_file(path)?;
        }
        path.parent().map(|p| fs::create_dir_all(p).unwrap());

        let listener = UnixListener::bind(path)?;
        info!("Listening on: {}", path.display());

        let state_1 = state.clone();
        join_set.spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.expect("accept failed");
                let state_2 = state_1.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(socket, state_2).await {
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
    CLIENT_MAPPINGS
        .get(client_id)
        .unwrap_or(&client_id)
        .to_string()
}

static EXPECTED_TOKEN: OnceLock<String> = OnceLock::new();
static UPSTREAM_URL: OnceLock<String> = OnceLock::new();

async fn periodic_send(state: SharedState) {
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        let state = state.lock().unwrap().clone();
        let mut payload_vec: Vec<presenced::PresenceState> = vec![];
        for (client_id, state) in state.iter() {
            payload_vec.push(presenced::PresenceState {
                client: client_id_to_name(client_id),
                large_text: state.large_text.clone(),
                small_text: state.small_text.clone(),
                state: state.state.clone(),
                details: state.details.clone(),
                start_time: state.start_time.map(DateTime::<Local>::from),
                end_time: state.end_time.map(DateTime::<Local>::from),
            });
        }
        // Send to endpoint /state
        let response = reqwest::Client::new()
            .post(format!("{}/state", UPSTREAM_URL.get().unwrap()))
            .json(&presenced::StateUpdate {
                token: EXPECTED_TOKEN.get().unwrap().to_string(),
                state: payload_vec,
            })
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        if let Err(e) = response {
            warn!("Error sending state: {:?}", e);
        }
    }
}

async fn handle_connection(
    mut socket: UnixStream,
    global_state: SharedState,
) -> Result<(), Box<dyn Error>> {
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

    async fn handle_inner(
        client_id: &str,
        mut socket: UnixStream,
        global_state: &SharedState,
    ) -> Result<(), Box<dyn Error>> {
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
                        global_state
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

    let res = handle_inner(&client_id, socket, &global_state).await;
    global_state.lock().unwrap().remove(&client_id);

    res
}
