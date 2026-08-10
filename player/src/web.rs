//! The web UI: state push over a WebSocket, controls over POST.
//!
//! The browser is a *view*. It holds no playback state of its own — every
//! change arrives as a fresh snapshot of [`PlayerState`], and every control is
//! a command the engine may act on or ignore. That is what keeps one answer to
//! "what is playing": the engine's, published once per tick `[REQ-AUD-142]`.
//!
//! The wire format is deliberately its own type rather than a `Serialize` on
//! [`QueueEntry`]. The browser contract should change when we decide it does,
//! not as a side effect of adding a field to an internal struct — and the
//! filesystem path is nobody's business outside the process.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::Router;
use serde::Serialize;

use crate::engine::{Command, EngineHandle, PlayerState};

/// How often a connected browser is sent a snapshot. Fast enough that the
/// position counter moves smoothly, slow enough to cost nothing on a Pi.
const PUSH_EVERY: Duration = Duration::from_millis(500);

/// What the browser is told. One flat object, so the client needs no merge
/// logic and cannot drift out of step with the engine.
#[derive(Serialize)]
pub struct Snapshot {
    pub playing: bool,
    pub title: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub queue_len: usize,
    pub underrun_samples: u64,
    /// Absent until the Program Director can explain itself `[REQ-VIS-100]`.
    /// Present and null, rather than omitted, so the panel can say "not yet
    /// available" instead of silently rendering nothing.
    pub why: Option<serde_json::Value>,
}

impl From<&PlayerState> for Snapshot {
    fn from(s: &PlayerState) -> Self {
        Snapshot {
            playing: s.playing,
            title: s.current.as_ref().map(|e| {
                e.path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            }),
            position_ms: s.position_ms,
            duration_ms: s.current.as_ref().map(|e| e.duration_ms()).unwrap_or(0),
            queue_len: s.queue_len,
            underrun_samples: s.underrun_samples,
            why: None,
        }
    }
}

pub fn router(handle: Arc<EngineHandle>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/ws", get(ws_upgrade))
        .route("/command/:name", post(command))
        .with_state(handle)
}

async fn ws_upgrade(
    State(h): State<Arc<EngineHandle>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| push_state(socket, h))
}

/// Push snapshots until the browser goes away. Each is independent, so a
/// dropped frame costs nothing and a reconnecting client needs no replay.
async fn push_state(mut socket: WebSocket, h: Arc<EngineHandle>) {
    let mut tick = tokio::time::interval(PUSH_EVERY);
    loop {
        tick.tick().await;
        let snap = Snapshot::from(&h.snapshot());
        let Ok(text) = serde_json::to_string(&snap) else { continue };
        if socket.send(Message::Text(text)).await.is_err() {
            return; // client gone
        }
    }
}

/// Controls are named rather than numbered so the wire stays readable and an
/// unknown name is a clean 404 rather than a silently wrong action.
async fn command(State(h): State<Arc<EngineHandle>>, axum::extract::Path(name): axum::extract::Path<String>) -> StatusCode {
    match name.as_str() {
        "play" => h.send(Command::Play),
        "pause" => h.send(Command::Pause),
        "skip" => h.send(Command::Skip),
        // Deliberately no "stop": there are two states [REQ-AUD-142].
        _ => return StatusCode::NOT_FOUND,
    }
    StatusCode::NO_CONTENT
}

const INDEX: &str = include_str!("web/index.html");
