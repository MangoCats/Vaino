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
/// The skip transition, with the limits it may be set between `[REQ-AUD-162]`.
///
/// The bounds travel with the values so the browser can offer exactly the range
/// the engine will accept, rather than keeping its own copy to fall out of step.
#[derive(Serialize)]
pub struct SkipShape {
    pub fade_ms: u64,
    pub lead_ms: u64,
    pub fade_max_ms: u64,
    pub lead_min_ms: u64,
    pub lead_max_ms: u64,
}

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
    /// Master level in dB relative to full scale, `-72.0` to `0.0`
    /// `[REQ-AUD-154]`. The control is graduated in dB and captioned with this
    /// number directly, so the browser never converts and cannot disagree with
    /// the engine about what the fader is set to.
    pub volume_db: f32,
    /// The bottom of the fader, in dB. Sent so the control can shape itself
    /// around the engine's floor instead of keeping its own copy of the number
    /// `[REQ-AUD-156]`.
    pub fader_min_db: f32,
    pub skip: SkipShape,
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
            volume_db: Volume::db_for(s.volume),
            fader_min_db: crate::output::FADER_MIN_DB,
            skip: SkipShape {
                fade_ms: s.skip_fade_ms,
                lead_ms: s.skip_lead_ms,
                fade_max_ms: crate::SKIP_FADE_MAX_MS,
                lead_min_ms: crate::SKIP_LEAD_MIN_MS,
                lead_max_ms: crate::SKIP_LEAD_MAX_MS,
            },
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
        .route("/", get(|| async { Html(SHELL) }))
        .route("/core.js", get(|| async { js(CORE) }))
        .route("/skins", get(skin_list))
        .route("/skin/:name/:file", get(skin_asset))
        .route("/ws", get(ws_upgrade))
        .route("/command/:name", post(command))
        .route("/volume/:db", post(set_volume))
        .route("/skip/fade/:ms", post(set_skip_fade))
        .route("/skip/lead/:ms", post(set_skip_lead))
        .route("/program/:id", post(set_program))
        .with_state(ui)
}

async fn ws_upgrade(State(ui): State<Ui>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| push_state(socket, ui))
}

/// Master level in dB relative to full scale, `-72.0` to `0.0`.
///
/// Fractional now that the control moves a pixel at a time `[REQ-AUD-156]`: at
/// the top of its travel a pixel is worth less than a hundredth of a dB, so
/// whole decibels would quantise away most of the resolution the curve exists
/// to provide. Rust parses a decimal point locale-independently, so the earlier
/// worry about one crossing a URL does not apply to the parser -- only to
/// anything that might have formatted it, and the browser formats with
/// `toFixed`.
///
/// It becomes an amplitude here, at the edge, so that everything inward of this
/// point -- engine, device, saved state -- speaks in amplitude, which is what
/// multiplies samples, and only the listener's control speaks in dB
/// `[REQ-AUD-154]`.
async fn set_volume(
    State(ui): State<Ui>,
    axum::extract::Path(db): axum::extract::Path<f32>,
) -> StatusCode {
    ui.handle.send(Command::SetVolume(Volume::amplitude_at_db(db)));
    StatusCode::NO_CONTENT
}

/// How long a skip fades the outgoing passage out, in ms. Clamped by the
/// engine, which owns the limits `[REQ-AUD-162]`.
async fn set_skip_fade(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipFade(ms));
    StatusCode::NO_CONTENT
}

/// How long after a skip the next passage starts, in ms `[REQ-AUD-162]`.
async fn set_skip_lead(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipLead(ms));
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

/// The page, and the skins it may wear `[REQ-VIS-160]`.
///
/// `include_str!` rather than reading from disk: one binary that runs anywhere
/// without a data directory beside it, which is what makes deploying to a Pi a
/// copy rather than an install. Adding a skin means adding a row here and three
/// files; nothing else in the server changes.
const SHELL: &str = include_str!("web/shell.html");
const CORE: &str = include_str!("web/core.js");

struct Skin {
    name: &'static str,
    label: &'static str,
    html: &'static str,
    css: &'static str,
    js: &'static str,
}

macro_rules! skin {
    ($name:literal, $label:literal) => {
        Skin {
            name: $name,
            label: $label,
            html: include_str!(concat!("web/skins/", $name, "/skin.html")),
            css: include_str!(concat!("web/skins/", $name, "/skin.css")),
            js: include_str!(concat!("web/skins/", $name, "/skin.js")),
        }
    };
}

const SKINS: &[Skin] = &[
    skin!("vaino", "Vaino"),
    skin!("mulibplay", "MuLibPlay"),
    skin!("winamp", "WinAmp"),
];

fn js(body: &'static str) -> impl IntoResponse {
    ([(axum::http::header::CONTENT_TYPE, "text/javascript; charset=utf-8")], body)
}

/// What the browser may choose between. The catalogue is served rather than
/// written into each skin, so adding one does not mean editing the others to
/// list it.
async fn skin_list() -> impl IntoResponse {
    let names: Vec<_> = SKINS
        .iter()
        .map(|s| serde_json::json!({ "name": s.name, "label": s.label }))
        .collect();
    axum::Json(names)
}

/// A skin is exactly three files. The set is fixed, so an unknown name is a
/// 404 rather than anything that could reach outside the binary.
async fn skin_asset(
    axum::extract::Path((name, file)): axum::extract::Path<(String, String)>,
) -> axum::response::Response {
    let Some(skin) = SKINS.iter().find(|s| s.name == name) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match file.as_str() {
        "skin.html" => Html(skin.html).into_response(),
        "skin.css" => (
            [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
            skin.css,
        )
            .into_response(),
        "skin.js" => js(skin.js).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}
