use std::error::Error;

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct PresenceState {
    pub client: String,
    pub large_text: String,
    pub small_text: String,
    pub state: String,
    pub details: String,
    pub start_time: Option<DateTime<Local>>,
    pub end_time: Option<DateTime<Local>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StateUpdate {
    pub token: String,
    pub state: Vec<PresenceState>,
}

#[derive(Debug)]
pub struct Message {
    pub opcode: u32,
    pub payload: serde_json::Value,
}

pub async fn socket_decode(socket: &mut UnixStream) -> Result<Message, Box<dyn Error>> {
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

pub async fn socket_encode(
    socket: &mut UnixStream,
    message: Message,
) -> Result<(), Box<dyn Error>> {
    let opcode = message.opcode.to_le_bytes();
    socket.write_all(&opcode).await?;
    let payload = serde_json::to_string(&message.payload)?;
    let length = payload.len() as u32;
    let length = length.to_le_bytes();
    socket.write_all(&length).await?;
    socket.write_all(payload.as_bytes()).await?;
    Ok(())
}
