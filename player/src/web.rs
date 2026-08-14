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
use crate::output::Volume;
use crate::session::{Explanations, SharedControls};

/// What the server needs to answer a request: the control surface, and why the
/// current passage was chosen.
#[derive(Clone)]
pub struct Ui {
    pub handle: Arc<EngineHandle>,
    pub why: Explanations,
    pub controls: SharedControls,
}

/// How often a connected browser is sent a snapshot. Fast enough that the
/// position counter moves smoothly, slow enough to cost nothing on a Pi.
const PUSH_EVERY: Duration = Duration::from_millis(500);

/// What the browser is told. One flat object, so the client needs no merge
/// logic and cannot drift out of step with the engine.
#[derive(Serialize)]
pub struct QueueItem {
    pub passage_id: i64,
    pub title: String,
    pub duration_ms: u64,
}

#[derive(Serialize)]
pub struct ProgramItem {
    pub id: i64,
    pub name: String,
    pub start: String,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub playing: bool,
    pub title: Option<String>,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub queue_len: usize,
    /// What is coming, in play order.
    pub queue: Vec<QueueItem>,
    /// Fader *travel*, 0.0 to 1.0 -- a knob position, not an amplitude. The
    /// taper between the two is `Volume::amplitude_at` `[REQ-AUD-154]`, and it
    /// lives there rather than here so the browser holds no audio constants.
    pub volume: f32,
    /// Level in dB, or `null` when the fader is closed.
    pub volume_db: Option<f32>,
    /// The programme in force, and whether it was chosen by hand.
    pub program: Option<String>,
    pub program_manual: bool,
    pub programs: Vec<ProgramItem>,
    pub underrun_samples: u64,
    /// The full weight decomposition for what is playing `[REQ-VIS-100]`.
    /// Null when the passage was not chosen by the Director -- a resumed
    /// passage, or one queued before the log was populated -- so the panel can
    /// say so rather than render an explanation that was never computed.
    pub why: Option<serde_json::Value>,
}

impl From<&PlayerState> for Snapshot {
    fn from(s: &PlayerState) -> Self {
        Snapshot {
            playing: s.playing,
            title: s.current.as_ref().map(|e| e.title()),
            position_ms: s.position_ms,
            duration_ms: s.current.as_ref().map(|e| e.duration_ms()).unwrap_or(0),
            queue_len: s.queue_len,
            queue: s
                .queue
                .iter()
                .map(|e| QueueItem {
                    passage_id: e.passage_id,
                    title: e.title(),
                    duration_ms: e.duration_ms(),
                })
                .collect(),
            volume: Volume::travel_for(s.volume),
            volume_db: Volume::db_at(Volume::travel_for(s.volume)),
            program: None,
            program_manual: false,
            programs: Vec::new(),
            underrun_samples: s.underrun_samples,
            why: None,
        }
    }
}

pub fn router(ui: Ui) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX) }))
        .route("/ws", get(ws_upgrade))
        .route("/command/:name", post(command))
        .route("/volume/:level", post(set_volume))
        .route("/program/:id", post(set_program))
        .with_state(ui)
}

async fn ws_upgrade(State(ui): State<Ui>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| push_state(socket, ui))
}

/// Fader position as a percentage of travel, 0-100. A percentage rather than a
/// float because it crosses a URL, and "50" is unambiguous where "0.5" invites
/// locale trouble.
///
/// The position becomes an amplitude here, at the edge, so that everything
/// inward of this point -- engine, device, saved state -- speaks in amplitude
/// and only the listener's control speaks in travel `[REQ-AUD-154]`.
async fn set_volume(
    State(ui): State<Ui>,
    axum::extract::Path(level): axum::extract::Path<u32>,
) -> StatusCode {
    let travel = level.min(100) as f32 / 100.0;
    ui.handle.send(Command::SetVolume(Volume::amplitude_at(travel)));
    StatusCode::NO_CONTENT
}

/// Choose a programme by hand, or `auto` to revert to time of day
/// `[SPEC-DIR-185]`. Applied by the engine on its next refill, so an override
/// changes what is selected NEXT rather than interrupting what is playing.
async fn set_program(
    State(ui): State<Ui>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> StatusCode {
    let Ok(mut c) = ui.controls.lock() else { return StatusCode::INTERNAL_SERVER_ERROR };
    if id == "auto" {
        c.manual_program = None;
        return StatusCode::NO_CONTENT;
    }
    match id.parse::<i64>() {
        Ok(n) if c.programs.iter().any(|p| p.0 == n) => {
            c.manual_program = Some(n);
            StatusCode::NO_CONTENT
        }
        _ => StatusCode::NOT_FOUND,
    }
}

/// Attach the reasoning for whatever is playing now.
fn explain(ui: &Ui, snap: &mut Snapshot, state: &PlayerState) {
    let Some(id) = state.current.as_ref().map(|e| e.passage_id) else { return };
    let Ok(log) = ui.why.lock() else { return };
    snap.why = log.get(id).and_then(|w| serde_json::to_value(w).ok());
}

/// Push snapshots until the browser goes away. Each is independent, so a
/// dropped frame costs nothing and a reconnecting client needs no replay.
async fn push_state(mut socket: WebSocket, ui: Ui) {
    let mut tick = tokio::time::interval(PUSH_EVERY);
    loop {
        tick.tick().await;
        let state = ui.handle.snapshot();
        let mut snap = Snapshot::from(&state);
        explain(&ui, &mut snap, &state);
        if let Ok(c) = ui.controls.lock() {
            snap.program = c.active.clone();
            snap.program_manual = c.manual_program.is_some();
            snap.programs = c
                .programs
                .iter()
                .map(|(id, name, start)| ProgramItem {
                    id: *id,
                    name: name.clone(),
                    start: start.clone(),
                })
                .collect();
        }
        let Ok(text) = serde_json::to_string(&snap) else { continue };
        if socket.send(Message::Text(text)).await.is_err() {
            return; // client gone
        }
    }
}

/// Controls are named rather than numbered so the wire stays readable and an
/// unknown name is a clean 404 rather than a silently wrong action.
async fn command(
    State(ui): State<Ui>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> StatusCode {
    let h = &ui.handle;
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
