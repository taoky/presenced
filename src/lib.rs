use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct PresenceState {
    pub client: String,
    pub large_text: String,
    pub small_text: String,
    pub state: String,
    pub details: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct StateUpdate {
    pub token: String,
    pub state: Vec<PresenceState>,
}
