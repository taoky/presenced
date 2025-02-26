use std::sync::{Arc, OnceLock};

use askama::Template;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use axum_macros::debug_handler;
use chrono::{DateTime, Local};
use tokio::sync::RwLock;

use presenced::{PresenceState, StateUpdate};

#[derive(Debug)]
struct AppState {
    states: RwLock<Vec<PresenceState>>,
    last_updated: RwLock<DateTime<Local>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            states: RwLock::new(Vec::new()),
            last_updated: RwLock::new(Local::now()),
        }
    }
}

#[derive(Template)]
#[template(path = "index.html")]
struct PresenceTemplate {
    states: Vec<PresenceState>,
    last_updated: DateTime<Local>,
}

#[debug_handler]
async fn show_state(State(app_state): State<Arc<AppState>>) -> impl IntoResponse {
    let states = app_state.states.read().await;
    let reply_html = PresenceTemplate {
        states: states.clone(),
        last_updated: *app_state.last_updated.read().await,
    }
    .render()
    .unwrap();
    axum::response::Html(reply_html).into_response()
}

static EXPECTED_TOKEN: OnceLock<String> = OnceLock::new();

#[debug_handler]
async fn update_state(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<StateUpdate>,
) -> StatusCode {
    if payload.token != *EXPECTED_TOKEN.get().unwrap() {
        return StatusCode::UNAUTHORIZED;
    }

    let mut guard = app_state.states.write().await;
    let state = &payload.state;
    guard.clear();
    guard.extend_from_slice(state);
    *app_state.last_updated.write().await = Local::now();

    StatusCode::OK
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();
    EXPECTED_TOKEN.get_or_init(|| std::env::var("TOKEN").expect("TOKEN env var not set"));
    let app_state = Arc::new(AppState::default());

    let app = Router::new()
        .route("/", get(show_state))
        .route("/state", post(update_state))
        .with_state(app_state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001").await.unwrap();
    println!("Listening on 0.0.0.0:3001");
    axum::serve(listener, app).await.unwrap();
}
