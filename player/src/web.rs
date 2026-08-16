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

use crate::engine::{Command, EngineHandle, Placement, PlayerState};
use crate::output::Volume;
use crate::session::{Explanations, SharedControls};

/// What the server needs to answer a request: the control surface, and why the
/// current passage was chosen.
#[derive(Clone)]
pub struct Ui {
    pub handle: Arc<EngineHandle>,
    /// The library file, for serving cover art. A path rather than a
    /// connection: `rusqlite`'s is not `Sync`, art is asked for once per track
    /// change, and opening one for that is cheaper than sharing one forever.
    pub db: std::path::PathBuf,
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
    /// How often the resume point is written `[REQ-VIS-155]`. Here rather than
    /// in a settings object of its own because the page already binds this
    /// group, and the bounds travel with the value for the same reason the
    /// skip bounds do -- so the control offers exactly what the engine accepts.
    pub resume_save_ms: u64,
    pub resume_save_min_ms: u64,
    pub resume_save_max_ms: u64,
}

#[derive(Serialize)]
pub struct QueueItem {
    pub passage_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: u64,
    /// Whether it can still be moved or dropped `[REQ-VIS-185]`. False once
    /// the mixer has it: its audio is already partly in the ring, so removing
    /// it from the queue would change nothing anyone could hear.
    pub editable: bool,
}

#[derive(Serialize)]
pub struct ProgramItem {
    pub id: i64,
    pub name: String,
    pub start: String,
}

/// What the listening surface shows `[REQ-VIS-150]`: the queue in play order,
/// the programme in force with its manual override, and master volume -- all of
/// it as one complete snapshot, so a render is a pure function of the last
/// message and cannot drift.
#[derive(Serialize)]
pub struct Snapshot {
    pub playing: bool,
    /// The passage on air, for fetching its cover at `/art/{id}`.
    pub passage_id: Option<i64>,
    /// MusicBrainz Recording title where there is one, then the file's tag,
    /// then the filename `[REQ-VIS-170]`.
    pub title: Option<String>,
    pub artist: Option<String>,
    /// The **Release** title. `None` until the release tables are populated
    /// and the file carries no album tag either.
    pub album: Option<String>,
    /// Which source each displayed name came from `[REQ-VIS-120]`.
    pub title_source: &'static str,
    pub artist_source: &'static str,
    pub album_source: &'static str,
    /// Plays of this **recording**, all-time, and when it was last heard.
    pub plays: i64,
    pub last_played: Option<i64>,
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
    /// Most tracks a browse request will return, so the page can say so
    /// without a second copy of the number `[REQ-VIS-180]`.
    pub browse_limit: usize,
    pub skip: SkipShape,
    /// The programme in force, and whether it was chosen by hand.
    pub program: Option<String>,
    pub program_manual: bool,
    pub programs: Vec<ProgramItem>,
    /// Development mode is on `[PI-SET-016]`: sshd and diagnostics are
    /// running. Always false off the appliance -- the setting belongs to the
    /// Pi's privileged helper, and the player only reports what it is told.
    pub dev_mode: bool,
    pub underrun_samples: u64,
    /// Output-lock contention `[REQ-VIS-140]`. Non-zero means the callback was
    /// kept waiting, which sounds like a click rather than a gap.
    pub lock_failures: u64,
    pub out_recoveries: u64,
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
            passage_id: s.current.as_ref().map(|e| e.passage_id),
            title: s.current.as_ref().map(|e| e.title()),
            artist: s.current.as_ref().and_then(|e| e.artist()),
            album: s.current.as_ref().and_then(|e| e.album()),
            title_source: s.current.as_ref().map_or("unknown", |e| e.title_source().as_str()),
            artist_source: s.current.as_ref().map_or("unknown", |e| e.artist_source().as_str()),
            album_source: s.current.as_ref().map_or("unknown", |e| e.album_source().as_str()),
            plays: s.current.as_ref().map_or(0, |e| e.naming.plays),
            last_played: s.current.as_ref().and_then(|e| e.naming.last_played),
            position_ms: s.position_ms,
            duration_ms: s.current.as_ref().map(|e| e.duration_ms()).unwrap_or(0),
            queue_len: s.queue_len,
            queue: s
                .queue
                .iter()
                .enumerate()
                .map(|(i, e)| QueueItem {
                    passage_id: e.passage_id,
                    title: e.title(),
                    artist: e.artist(),
                    duration_ms: e.duration_ms(),
                    editable: i >= s.mixing_ahead,
                })
                .collect(),
            volume_db: Volume::db_for(s.volume),
            fader_min_db: crate::output::FADER_MIN_DB,
            browse_limit: crate::BROWSE_LIMIT,
            skip: SkipShape {
                fade_ms: s.skip_fade_ms,
                lead_ms: s.skip_lead_ms,
                fade_max_ms: crate::SKIP_FADE_MAX_MS,
                lead_min_ms: crate::SKIP_LEAD_MIN_MS,
                lead_max_ms: crate::SKIP_LEAD_MAX_MS,
                resume_save_ms: s.resume_save_ms,
                resume_save_min_ms: crate::RESUME_SAVE_MIN_MS,
                resume_save_max_ms: crate::RESUME_SAVE_MAX_MS,
            },
            program: None,
            program_manual: false,
            programs: Vec::new(),
            dev_mode: s.dev_mode,
            underrun_samples: s.underrun_samples,
            lock_failures: s.lock_failures,
            out_recoveries: s.out_recoveries,
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
        .route("/why/:passage_id", get(why_for))
        .route("/art/:passage_id", get(cover_art))
        .route("/art/:passage_id/back", get(cover_art_back))
        .route("/browse", get(|| async { ([REVALIDATE], Html(BROWSE_HTML)) }))
        .route("/browse.js", get(|| async { js(BROWSE_JS) }))
        .route("/browse/:kind", get(browse))
        .route("/review", get(|| async { ([REVALIDATE], Html(REVIEW_HTML)) }))
        .route("/review.js", get(|| async { js(REVIEW_JS) }))
        .route(REVIEW_QUEUE_ROUTE, get(review_queue))
        .route("/review/releases/:mbid", get(review_releases))
        .route("/review/:passage_id/:decision", post(record_review))
        .route("/queue/:passages/:action", post(queue_passage))
        .route("/ws", get(ws_upgrade))
        .route("/audio/sink", get(audio_sink))
        .route("/command/:name", post(command))
        .route("/volume/:db", post(set_volume))
        .route("/skip/fade/:ms", post(set_skip_fade))
        .route("/skip/lead/:ms", post(set_skip_lead))
        .route("/resume/save/:ms", post(set_resume_save))
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

/// Browse the library by artist, album or track `[REQ-VIS-180]`.
///
/// Read-only and off the engine entirely: the browse page asks the database
/// directly rather than going through the player, so listing ten thousand
/// tracks cannot get in the way of playing one.
async fn browse(
    State(ui): State<Ui>,
    axum::extract::Path(kind): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let filter = crate::db::BrowseFilter {
        q: q.get("q").filter(|s| !s.is_empty()).cloned(),
        artist: q.get("artist").filter(|s| !s.is_empty()).cloned(),
        album: q.get("album").filter(|s| !s.is_empty()).cloned(),
    };
    let out = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        match kind.as_str() {
            "artists" => serde_json::to_value(lib.browse_artists(&filter).ok()?).ok(),
            "albums" => serde_json::to_value(lib.browse_albums(&filter).ok()?).ok(),
            "tracks" => serde_json::to_value(lib.browse_tracks(&filter).ok()?).ok(),
            // The row cap, so the page can report it without a second copy.
            "limit" => serde_json::to_value(crate::BROWSE_LIMIT).ok(),
            _ => None,
        }
    })
    .await;
    match out {
        Ok(Some(v)) => axum::Json(v).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Act on a passage's place in the queue `[REQ-VIS-185]`.
///
/// Six verbs on one route, because they are one idea -- where does this go --
/// and splitting them across six routes would spread that idea thin:
///
/// * `now` — to the front, then skip into it. The only one that interrupts.
/// * `next` — after the current passage.
/// * `last` — to the back, behind everything already waiting.
/// * `remove` — out of the queue.
/// * `sooner` / `later` — one place each way, clamped at the ends.
///
/// The first three take a passage from the library; the last three act on one
/// already queued, and need no library read at all.
async fn queue_passage(
    State(ui): State<Ui>,
    axum::extract::Path((passages, action)): axum::extract::Path<(String, String)>,
) -> StatusCode {
    // A comma-separated list, so one selected track and thirty travel the same
    // path `[REQ-VIS-195]`. They must arrive together: three passages sent as
    // three requests and inserted one at a time at the same place come out
    // backwards, which looks like a UI fault and is not.
    let ids: Vec<i64> = passages.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    let Some(&passage_id) = ids.first() else {
        return StatusCode::NOT_FOUND;
    };
    // Editing the queue is a rearrangement, not a lookup: the passage is
    // already there, so this must not touch the database.
    let place = match action.as_str() {
        "remove" => {
            for id in &ids {
                ui.handle.send(Command::RemoveQueued(*id));
            }
            return StatusCode::NO_CONTENT;
        }
        "sooner" => {
            ui.handle.send(Command::ShiftQueued(passage_id, -1));
            return StatusCode::NO_CONTENT;
        }
        "later" => {
            ui.handle.send(Command::ShiftQueued(passage_id, 1));
            return StatusCode::NO_CONTENT;
        }
        "now" => Placement::Now,
        "next" => Placement::Next,
        "last" => Placement::Last,
        _ => return StatusCode::NOT_FOUND,
    };
    let db = ui.db.clone();
    let entries = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        // Order is the caller's, and it is the order they were looking at.
        // A passage that cannot be read is dropped rather than failing the
        // batch: nineteen tracks queued beats none.
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Ok(mut e) = lib.passage(id) {
                lib.describe(&mut e);
                if let Some(t) = lib.stored_tags(id) {
                    e.naming.apply_tags(t);
                }
                out.push(e);
            }
        }
        Some(out)
    })
    .await;
    match entries {
        Ok(Some(v)) if !v.is_empty() => {
            ui.handle.send(Command::EnqueueMany(v, place));
            StatusCode::NO_CONTENT
        }
        _ => StatusCode::NOT_FOUND,
    }
}

/// The questionable ids, with the evidence against them `[REQ-LIB-165]`.
///
/// Progress travels with the list so the page can distinguish three states that
/// would otherwise all render as an empty table: the pass has never been run,
/// it ran and found nothing, or everything it found has been dealt with.
async fn review_queue(State(ui): State<Ui>) -> axum::response::Response {
    let db = ui.db.clone();
    let out = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        let items = lib.review_queue(crate::BROWSE_LIMIT).ok()?;
        serde_json::to_value(serde_json::json!({
            "progress": lib.review_progress(),
            "items": items,
        }))
        .ok()
    })
    .await;
    match out {
        Ok(Some(v)) => axum::Json(v).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Record a judgement `[REQ-LIB-165]`.
///
/// `kept`, `reassigned` or `deferred`; a reassignment carries `?mbid=`. The
/// vocabulary is checked in `PlayerStore`, which is the only writer, so an
/// unknown verb is rejected there rather than trusted from the URL.
///
/// Nothing here rewrites `passage_recordings`. A decision is recorded, and
/// `tools/apply_reviews.py` folds accepted ones into the library as a separate,
/// deliberate step -- reassigning an id changes what a passage *is*, and play
/// history is keyed by recording.
async fn record_review(
    State(ui): State<Ui>,
    axum::extract::Path((passage_id, decision)): axum::extract::Path<(i64, String)>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let mbid = q.get("mbid").cloned();
    let release = q.get("release").cloned();
    let done = tokio::task::spawn_blocking(move || {
        let store = crate::db::PlayerStore::open(&db)
            .map_err(|e| e.message().to_string())?;
        // `reopen` is the undo. It is a decision verb like the others from the
        // page's point of view, and a different operation underneath, so it
        // routes here rather than growing a second endpoint shape.
        if decision == "reopen" {
            store.clear_review(passage_id).map_err(|e| e.message().to_string())
        } else {
            store
                .record_review(passage_id, &decision, mbid.as_deref(), release.as_deref())
                .map_err(|e| e.message().to_string())
        }
    })
    .await;
    match done {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        // The reason travels back as text. A refusal a person cannot read is
        // one they will retry, and "already applied to the library" is
        // precisely what they need to be told.
        Ok(Err(why)) => (StatusCode::CONFLICT, why).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The releases a recording appears on `[REQ-LIB-165]`, for naming the album.
///
/// Fetched when a candidate is chosen rather than shipped with the queue: a
/// recording can be on dozens of releases, and sending them for every
/// candidate of every card would be most of the payload for something almost
/// none of them will be asked about.
async fn review_releases(
    State(ui): State<Ui>,
    axum::extract::Path(mbid): axum::extract::Path<String>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let out = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        serde_json::to_value(lib.releases_for(&mbid).ok()?).ok()
    })
    .await;
    match out {
        Ok(Some(v)) => axum::Json(v).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Why any one passage was chosen `[REQ-VIS-100]`.
///
/// The log is keyed by passage and already holds the queued ones; only the
/// playing one was ever reachable. A skin that lets you ask about the track
/// *after* next needs the rest of it.
///
/// Fetched on demand rather than pushed with every snapshot: an explanation is
/// several hundred bytes of weights and runners-up, it never changes once the
/// passage has been chosen, and pushing six of them twice a second to render
/// one would be most of the traffic.
///
/// 404 when there is none -- a resumed passage, or one queued before the log
/// existed. That is a real state and the page says so rather than blanking.
async fn why_for(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    let Ok(log) = ui.why.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    match log.get(passage_id).and_then(|w| serde_json::to_value(w).ok()) {
        Some(v) => axum::Json(v).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// How often the resume point is written `[REQ-VIS-155]`.
async fn set_resume_save(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetResumeSave(ms));
    StatusCode::NO_CONTENT
}

/// The passage's cover art `[REQ-VIS-170]`.
///
/// Read from the audio file, never fetched: playback must not depend on a live
/// external service `[REQ-NEG-100]`, and the Cover Art Archive is precisely the
/// dependency that forbids. Files without a picture are a plain 404, which is
/// what lets a skin ask unconditionally and hide the element on failure.
///
/// Served by passage rather than by album because that is the id a skin has in
/// hand, and it makes the URL stable enough to cache for a day.
async fn cover_art(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    // Both the query and the file read block, so they belong off the runtime.
    art_response(ui, passage_id, false).await
}

/// The back of the sleeve `[REQ-VIS-170]`, for skins that show it.
async fn cover_art_back(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    art_response(ui, passage_id, true).await
}

/// One passage's cover, from wherever it can be found.
///
/// Three sources, cheapest first `[REQ-VIS-170]`:
///
/// 1. **the audio file's own picture** -- 64% of this library carries one;
/// 2. **a cover file beside it** -- `folder.jpg` and its spellings, which 83%
///    of the remaining files have. It was already on disk and nothing looked;
/// 3. **the fetched archive**, keyed by the release Sampo chose, which is the
///    only one of the three that can tell two albums in one folder apart.
///
/// A back cover skips step 1: embedded back covers are vanishingly rare, and
/// `artwork()` deliberately falls back to *any* picture, which would return
/// the front and label it the back.
async fn art_response(ui: Ui, passage_id: i64, back: bool) -> axum::response::Response {
    let db = ui.db.clone();
    let found = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        let path = lib.passage_path(passage_id).ok();
        if !back {
            if let Some(a) = path.as_deref().and_then(crate::tags::artwork) {
                if a.data.len() >= crate::tags::MIN_ART_BYTES {
                    return Some(a);
                }
            }
        }
        if let Some(a) = path.as_deref().and_then(|p| crate::tags::sibling_art(p, back)) {
            return Some(a);
        }
        lib.stored_art(passage_id, back)
    })
    .await;

    match found {
        Ok(Some(art)) => (
            [
                (axum::http::header::CONTENT_TYPE, art.media_type),
                // The art of a given passage does not change; let the browser
                // keep it rather than re-reading the file on every track change.
                (axum::http::header::CACHE_CONTROL, "public, max-age=86400".into()),
            ],
            art.data,
        )
            .into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
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

/// Where the audio is actually going `[PI3-API-020]`.
///
/// On demand rather than in the state snapshot: it costs a subprocess, and the
/// settings panel is the only thing that needs it.
async fn audio_sink() -> axum::Json<crate::sink::SinkStatus> {
    axum::Json(crate::sink::current())
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
        // Named rather than folded into a settings write because it is an
        // action with an audible consequence, not a stored preference: the
        // speaker panel calls it after changing the default sink, without
        // which the change is silent `[PI3-API-010]`.
        "reopen-output" => h.send(Command::ReopenOutput),
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
const BROWSE_HTML: &str = include_str!("web/browse.html");
const BROWSE_JS: &str = include_str!("web/browse.js");
const REVIEW_HTML: &str = include_str!("web/review.html");
const REVIEW_JS: &str = include_str!("web/review.js");

/// The route the review page fetches its work from, named once so the router
/// and the test that checks the page agrees with it cannot drift apart.
const REVIEW_QUEUE_ROUTE: &str = "/review/queue";
/// The prefix every decision is posted to. The page builds the rest of the
/// path from the passage id, so only the stem can be shared.
#[cfg(test)]
const REVIEW_DECIDE_PREFIX: &str = "/review/";
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

/// Skins are compiled into the binary, so a cached copy in the browser can be
/// older than the running player -- which looks exactly like a feature that
/// does not work. `no-cache` means revalidate, not "do not store": the browser
/// still keeps it and still gets a 304 when nothing changed.
const REVALIDATE: (axum::http::HeaderName, &str) =
    (axum::http::header::CACHE_CONTROL, "no-cache");

fn js(body: &'static str) -> impl IntoResponse {
    (
        [
            (axum::http::header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            REVALIDATE,
        ],
        body,
    )
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
        "skin.html" => ([REVALIDATE], Html(skin.html)).into_response(),
        "skin.css" => (
            [
                (axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8"),
                REVALIDATE,
            ],
            skin.css,
        )
            .into_response(),
        "skin.js" => js(skin.js).into_response(),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::QueueEntry;
    use std::path::PathBuf;

    fn entry(id: i64, title: &str) -> QueueEntry {
        let mut e = QueueEntry {
            passage_id: id,
            path: PathBuf::from(format!("/music/{title}.mp3")),
            start_ms: 0,
            end_ms: 200_000,
            lead_in_ms: 0,
            lead_out_ms: 0,
            gain_db: 0.0,
            mbid: None,
            naming: Default::default(),
        };
        e.naming.mb_title = Some(title.into());
        e
    }

    fn state() -> PlayerState {
        let mut s = PlayerState::default();
        let mut cur = entry(1, "Now Playing");
        cur.naming.mb_artist = Some("The Artist".into());
        cur.naming.tag_album = Some("Some Record".into());
        cur.naming.plays = 12;
        cur.naming.last_played = Some(1_700_000_000);
        s.current = Some(cur);
        s.queue = vec![entry(2, "Mixing"), entry(3, "Waiting")];
        s.mixing_ahead = 1;
        s.queue_len = 2;
        s.volume = 0.5;
        s.skip_fade_ms = 2_000;
        s.skip_lead_ms = 500;
        s
    }

    /// Every skin is three non-empty files under a unique name. A skin that
    /// compiles in empty would serve a blank page and fail nowhere else.
    #[test]
    fn every_registered_skin_is_complete() {
        assert!(!SKINS.is_empty());
        let mut seen = std::collections::HashSet::new();
        for s in SKINS {
            assert!(seen.insert(s.name), "duplicate skin name {}", s.name);
            assert!(!s.label.is_empty(), "{} has no label", s.name);
            for (what, body) in [("html", s.html), ("css", s.css), ("js", s.js)] {
                assert!(body.len() > 50, "{} {what} is empty", s.name);
            }
            // The contract: a skin gets its transport wired by core, so it must
            // mark the buttons up `[REQ-VIS-160]`.
            for cmd in ["play", "pause", "skip"] {
                assert!(
                    s.html.contains(&format!("data-cmd=\"{cmd}\"")),
                    "{} is missing the {cmd} control",
                    s.name
                );
            }
            assert!(s.html.contains("data-skins"), "{} has no skin picker", s.name);
        }
    }

    /// The shell and browse page must load core, or nothing on them works.
    #[test]
    fn the_pages_load_the_runtime() {
        assert!(SHELL.contains("/core.js"));
        assert!(SHELL.contains("Vaino.start()"), "the shell starts core");
        assert!(BROWSE_HTML.contains("/core.js") && BROWSE_HTML.contains("/browse.js"));
        assert!(BROWSE_JS.contains("startBare"), "browse takes the skin, not the player");
        assert!(REVIEW_HTML.contains("/core.js") && REVIEW_HTML.contains("/review.js"));
        assert!(REVIEW_JS.contains("startBare"), "review takes the skin, not the player");
        // Reviewing ids is reachable, or it may as well not exist -- it is not
        // linked from the player, deliberately, so browse is the only way in.
        assert!(BROWSE_HTML.contains("/review"), "no way to reach the review page");
    }

    /// The review page and its routes have to agree about the URLs, which is
    /// the seam a jsdom check cannot see: it mocks `fetch`, so a page asking
    /// for a route the server never registered still passes there.
    #[test]
    fn the_review_routes_match_what_the_page_asks_for() {
        assert!(REVIEW_JS.contains(REVIEW_QUEUE_ROUTE));
        assert!(REVIEW_JS.contains(REVIEW_DECIDE_PREFIX));
        // The three decisions the page can send, and the only three
        // `PlayerStore::record_review` accepts. One added on either side alone
        // is the bug this is here to catch.
        for decision in ["reassigned", "kept", "deferred"] {
            assert!(REVIEW_JS.contains(decision), "page cannot send {decision}");
            assert!(
                crate::db::PlayerStore::open(&std::path::PathBuf::from(":memory:"))
                    .map(|s| s.record_review(1, decision, Some("m"), None).is_ok())
                    .unwrap_or(false),
                "the store rejects {decision}, which the page can send"
            );
        }
    }

    /// Names reach the browser resolved, and carry where they came from
    /// `[REQ-VIS-170]`.
    #[test]
    fn a_snapshot_reports_names_and_their_provenance() {
        let snap = Snapshot::from(&state());
        assert_eq!(snap.title.as_deref(), Some("Now Playing"));
        assert_eq!(snap.artist.as_deref(), Some("The Artist"));
        assert_eq!(snap.album.as_deref(), Some("Some Record"));
        assert_eq!(snap.title_source, "musicbrainz");
        assert_eq!(snap.artist_source, "musicbrainz");
        // The release tables are empty, so album comes from the file and must
        // say so rather than claiming MusicBrainz.
        assert_eq!(snap.album_source, "tags");
        assert_eq!(snap.plays, 12);
        assert_eq!(snap.passage_id, Some(1));
    }

    /// What the mixer already holds cannot be edited, and the flag says which
    /// `[REQ-VIS-185]`.
    #[test]
    fn queue_items_report_whether_they_can_still_be_edited() {
        let snap = Snapshot::from(&state());
        assert_eq!(snap.queue.len(), 2);
        assert!(!snap.queue[0].editable, "the mixer has this one");
        assert!(snap.queue[1].editable, "this one is still only queued");
        assert_eq!(snap.queue[0].title, "Mixing");
    }

    /// Amplitude is internal; the browser is told dB and the floor
    /// `[REQ-AUD-154]`.
    #[test]
    fn volume_crosses_as_decibels_not_amplitude() {
        let snap = Snapshot::from(&state());
        assert!((snap.volume_db - (-6.0206)).abs() < 1e-3, "{}", snap.volume_db);
        assert_eq!(snap.fader_min_db, crate::output::FADER_MIN_DB);
    }

    /// The limits travel with the values, so the browser cannot offer a range
    /// the engine will refuse `[REQ-AUD-162]`.
    #[test]
    fn the_skip_shape_carries_its_own_limits() {
        let snap = Snapshot::from(&state());
        assert_eq!(snap.skip.fade_ms, 2_000);
        assert_eq!(snap.skip.lead_ms, 500);
        assert_eq!(snap.skip.fade_max_ms, crate::SKIP_FADE_MAX_MS);
        assert_eq!(snap.skip.lead_min_ms, crate::SKIP_LEAD_MIN_MS);
        assert_eq!(snap.skip.lead_max_ms, crate::SKIP_LEAD_MAX_MS);
    }

    /// An idle player must serialise rather than panic on its empty fields.
    #[test]
    fn an_empty_snapshot_is_still_a_snapshot() {
        let snap = Snapshot::from(&PlayerState::default());
        assert_eq!(snap.title, None);
        assert_eq!(snap.title_source, "unknown");
        assert_eq!(snap.plays, 0);
        assert!(snap.queue.is_empty());
        let json = serde_json::to_string(&snap).expect("an idle snapshot must serialise");
        assert!(json.contains("\"volume_db\""));
    }

    /// The wire shape the skins are written against. Renaming a field here
    /// silently blanks part of every skin, which no Rust test would otherwise
    /// catch `[REQ-VIS-160]`.
    #[test]
    fn the_snapshot_keeps_the_field_names_the_skins_read() {
        let json = serde_json::to_string(&Snapshot::from(&state())).unwrap();
        for field in [
            "playing", "passage_id", "title", "artist", "album", "plays",
            "last_played", "position_ms", "duration_ms", "queue_len", "queue",
            "volume_db", "fader_min_db", "skip", "program", "program_manual",
            "programs", "underrun_samples",
        ] {
            assert!(json.contains(&format!("\"{field}\"")), "snapshot lost {field}");
        }
        for field in ["passage_id", "title", "artist", "duration_ms", "editable"] {
            assert!(json.contains(&format!("\"{field}\"")), "queue item lost {field}");
        }
    }
}
