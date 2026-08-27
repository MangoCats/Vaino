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
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;

use crate::bluetooth;
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
    /// Skip suppression `[SPEC-PLAY-050]`, with its bounds so the control can
    /// draw itself without hard-coding them.
    pub skip_suppress_h: u64,
    pub skip_suppress_min_h: u64,
    pub skip_suppress_max_h: u64,
    /// Queue-removal suppression `[SPEC-PLAY-055]`, with its bounds.
    pub dequeue_suppress_h: u64,
    pub dequeue_suppress_min_h: u64,
    pub dequeue_suppress_max_h: u64,
    /// Passages kept ahead, and the guest sampling rate `[SPEC-MPD-105]`.
    pub queue_depth: usize,
    pub queue_depth_min: usize,
    pub queue_depth_max: usize,
    pub sample_interval_ms: u64,
    pub sample_interval_min_ms: u64,
    pub sample_interval_max_ms: u64,
}

#[derive(Serialize)]
pub struct QueueItem {
    /// The entry's own identity `[REQ-VIS-186]`. What the edit controls name,
    /// because `passage_id` does not distinguish a passage queued twice.
    pub qid: u64,
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
    /// What a live Director rebuild is doing, if one has been asked for
    /// `[IMPL-SUI-075]`. `None` until something requests one.
    /// What this server is `[REQ-VIS-200]`. Sent to every skin so a person
    /// looking at a page can say which build drew it, rather than being asked
    /// to remember.
    pub build: String,
    pub reload_status: Option<String>,
    /// Which backend is playing, whether a guest exists, and what the last
    /// switch did `[SPEC-BK-025]`.
    pub backend: Option<String>,
    pub guest_available: bool,
    pub guest_name: Option<String>,
    pub switch_status: Option<String>,
    /// `[REQ-VIS-205]`
    pub cue_sheets: bool,
    pub cue_status: Option<String>,
    /// `[REQ-VIS-210]`
    pub covers: bool,
    /// Whether the live backend can move inside a passage `[REQ-VIS-225]`.
    /// The bar is a control only where this is true.
    pub can_seek: bool,
    pub covers_status: Option<String>,
    /// `[REQ-VIS-215]`
    pub lyrics_cache: bool,
    pub lyrics_status: Option<String>,
    /// `[REQ-VIS-220]`
    pub lyrics_sidecar: bool,
    pub sidecar_status: Option<String>,
    /// The Director's pool as `(eligible, total)`, so a rebuild's effect is
    /// visible rather than asserted: importing music and reloading moves
    /// `total`. Absent when there is no Director.
    pub pool: Option<(usize, usize)>,
    pub programs: Vec<ProgramItem>,
    /// Development mode is on `[PI-SET-016]`: sshd and diagnostics are
    /// running. Always false off the appliance -- the setting belongs to the
    /// Pi's privileged helper, and the player only reports what it is told.
    pub dev_mode: bool,
    pub underrun_samples: u64,
    /// What the interface shows: the count since the baseline, and when that
    /// baseline was taken `[REQ-VIS-230]`.
    pub underruns_since_reset: u64,
    pub underruns_since: i64,
    /// Where the sound is actually coming from `[REQ-VIS-235]`: the file, and
    /// the passage's place in it.
    ///
    /// **Local only.** This names the listener's filesystem, and it belongs in
    /// the snapshot a local browser reads and nowhere that travels
    /// `[SPEC-DF-055]`.
    pub file_path: Option<String>,
    pub file_start_ms: u64,
    pub file_end_ms: u64,
    pub file_ms: u64,
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
                    qid: e.qid,
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
                skip_suppress_h: s.skip_suppress_h,
                skip_suppress_min_h: crate::SKIP_SUPPRESS_MIN_H,
                skip_suppress_max_h: crate::SKIP_SUPPRESS_MAX_H,
                dequeue_suppress_h: s.dequeue_suppress_h,
                dequeue_suppress_min_h: crate::DEQUEUE_SUPPRESS_MIN_H,
                dequeue_suppress_max_h: crate::DEQUEUE_SUPPRESS_MAX_H,
                queue_depth: s.queue_depth,
                queue_depth_min: crate::QUEUE_DEPTH_MIN,
                queue_depth_max: crate::QUEUE_DEPTH_MAX,
                sample_interval_ms: s.sample_interval_ms,
                sample_interval_min_ms: crate::SAMPLE_INTERVAL_MIN_MS,
                sample_interval_max_ms: crate::SAMPLE_INTERVAL_MAX_MS,
            },
            program: None,
            build: crate::build_id(),
            reload_status: None,
            backend: None,
            guest_available: false,
            guest_name: None,
            switch_status: None,
            cue_status: None,
            covers_status: None,
            lyrics_status: None,
            sidecar_status: None,
            pool: None,
            program_manual: false,
            programs: Vec::new(),
            cue_sheets: s.cue_sheets,
            covers: s.covers,
            // Replaced below from the live backend; the engine cannot answer for
            // a side that is not it.
            can_seek: false,
            lyrics_cache: s.lyrics_cache,
            lyrics_sidecar: s.lyrics_sidecar,
            dev_mode: s.dev_mode,
            underrun_samples: s.underrun_samples,
            underruns_since_reset: s.underruns_since_reset,
            underruns_since: s.underruns_since,
            file_path: s.current.as_ref().map(|e| e.path.to_string_lossy().to_string()),
            file_start_ms: s.current.as_ref().map_or(0, |e| e.start_ms),
            file_end_ms: s.current.as_ref().map_or(0, |e| e.end_ms),
            file_ms: s.current.as_ref().map_or(0, |e| e.file_ms),
            lock_failures: s.lock_failures,
            out_recoveries: s.out_recoveries,
            why: None,
        }
    }
}

pub fn router(ui: Ui) -> Router {
    let router = Router::new()
        .route("/", get(|| async { Html(SHELL) }))
        .route("/core.js", get(|| async { js(CORE) }))
        .route("/skins", get(skin_list))
        .route("/skin/:name/:file", get(skin_asset))
        .route("/why/:passage_id", get(why_for))
        .route("/lyrics/:passage_id", get(lyrics))
        .route("/art/:passage_id", get(cover_art))
        .route("/art/:passage_id/back", get(cover_art_back))
        .route("/browse", get(|| async { ([REVALIDATE], Html(BROWSE_HTML)) }))
        .route("/browse.js", get(|| async { js(BROWSE_JS) }))
        .route("/browse/:kind", get(browse))
        .route("/history", get(history));

    // The identification-review page, and everything reached from it: desktop
    // induct tooling with no reason to occupy an appliance image that never
    // runs Sampo `[SPEC-SUI-190]`. A build without this feature serves none of
    // these routes at all, not even a 404 stub -- the handlers, the embedded
    // HTML/JS and the database code behind them are not compiled in.
    #[cfg(feature = "sampo-support")]
    let router = router
        .route("/review", get(|| async { ([REVALIDATE], Html(REVIEW_HTML)) }))
        .route("/review.js", get(|| async { js(REVIEW_JS) }))
        .route(REVIEW_QUEUE_ROUTE, get(review_queue))
        .route("/review/releases/:mbid", get(review_releases))
        .route("/review/:passage_id/:decision", post(record_review))
        .route("/api/musicbrainz/search", get(musicbrainz_search))
        .route("/edit/:passage_id", get(|| async { ([REVALIDATE], Html(EDIT_HTML)) }))
        .route("/edit.js", get(|| async { js(EDIT_JS) }))
        .route("/fade.js", get(|| async { js(FADE_JS) }))
        .route("/edit/:passage_id/info", get(edit_info))
        .route("/edit/:passage_id/audio", get(edit_audio))
        .route("/edit/:passage_id/review", post(edit_review));

    router
        .route("/queue/:passages/:action", post(queue_passage))
        .route("/ws", get(ws_upgrade))
        .route("/audio/sink", get(audio_sink))
        .route("/audio/speakers", get(speakers))
        .route("/audio/speakers/:verb", post(speaker_verb))
        .route("/audio/speakers/:verb/:address", post(speaker_verb_on))
        .route("/command/:name", post(command))
        .route("/volume/:db", post(set_volume))
        .route("/seek/:ms", post(seek_to))
        .route("/underruns/restart", post(restart_underruns))
        .route("/skip/fade/:ms", post(set_skip_fade))
        .route("/skip/lead/:ms", post(set_skip_lead))
        .route("/resume/save/:ms", post(set_resume_save))
        .route("/skip/suppress/:hours", post(set_skip_suppress))
        .route("/dequeue/suppress/:hours", post(set_dequeue_suppress))
        .route("/queue/depth/:n", post(set_queue_depth))
        .route("/sample/interval/:ms", post(set_sample_interval))
        .route("/program/:id", post(set_program))
        .route("/library/reload", post(reload_library))
        .route("/backend/:which", post(switch_backend))
        .route("/cue/:on", post(set_cue_sheets))
        .route("/covers/:on", post(set_covers))
        .route("/lyricscache/:on", post(set_lyrics_cache))
        .route("/lyricssidecar/:on", post(set_lyrics_sidecar))
        .route("/audio/radios", get(radios))
        .route("/power/off", post(power_off))
        .route("/power/restart", post(restart_player))
        .route("/audio/radio/:kind/:state", post(set_radio))
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

/// Start the underrun count again from now `[REQ-VIS-230]`.
///
/// **The counter itself is not reset.** The engine moves a baseline and
/// reports the difference, so whatever wants the whole-process figure can
/// still have it — and a display that has been restarted does not make the
/// player forget it ever glitched.
async fn restart_underruns(State(ui): State<Ui>) -> StatusCode {
    ui.handle.send(Command::RestartUnderruns);
    StatusCode::NO_CONTENT
}

/// Move to a point inside the passage that is sounding `[REQ-VIS-225]`.
///
/// **In milliseconds into the passage, not a fraction of it.** The browser
/// knows where it clicked as a proportion of a bar, and could have sent that
/// — but a fraction means nothing without the duration, and the two sides
/// would then have to agree about which duration. The engine owns the span;
/// the browser converts once, here, against the duration it was shown.
async fn seek_to(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    let Ok(mut c) = ui.controls.lock() else { return StatusCode::INTERNAL_SERVER_ERROR };
    c.seek_requested = Some(ms);
    StatusCode::ACCEPTED
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

/// The page sizes the history panel offers `[REQ-VIS-250]`. Anything else
/// asked for falls back to the default rather than handing SQLite an
/// unbounded `LIMIT`.
const HISTORY_PAGE_SIZES: [i64; 3] = [10, 100, 1000];
const HISTORY_DEFAULT_SIZE: i64 = 100;

/// One page of the play-history panel, with enough to draw the pager without
/// a second request `[REQ-VIS-250]`.
#[derive(Serialize)]
struct HistoryPage {
    entries: Vec<crate::db::HistoryEntry>,
    total: i64,
    page: i64,
    size: i64,
}

/// What has actually sounded, paged `[REQ-VIS-250]`.
///
/// Off the engine entirely, like `browse`: a history read must never get in
/// the way of playing the next track.
async fn history(
    State(ui): State<Ui>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let size = q
        .get("size")
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| HISTORY_PAGE_SIZES.contains(n))
        .unwrap_or(HISTORY_DEFAULT_SIZE);
    let page = q.get("page").and_then(|s| s.parse::<i64>().ok()).unwrap_or(1).max(1);
    let offset = (page - 1) * size;
    let out = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        let entries = lib.play_history(size, offset).ok()?;
        let total = lib.play_history_count().ok()?;
        Some(HistoryPage { entries, total, page, size })
    })
    .await;
    match out {
        Ok(Some(page)) => axum::Json(page).into_response(),
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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
    // Two identifier spaces meet on this route, and conflating them was a bug
    // `[REQ-VIS-186]`. The verbs that ADD name passages in the library; the
    // verbs that EDIT name entries already in the queue, which is not the same
    // thing the moment a passage appears twice.
    //
    // Editing the queue is a rearrangement, not a lookup: what is named is
    // already there, so none of these touch the database.
    match action.as_str() {
        "remove" => {
            let qids: Vec<u64> =
                passages.split(',').filter_map(|s| s.trim().parse().ok()).collect();
            if qids.is_empty() {
                return StatusCode::NOT_FOUND;
            }
            for qid in qids {
                ui.handle.send(Command::RemoveQueued(qid));
            }
            return StatusCode::NO_CONTENT;
        }
        "sooner" | "later" => {
            let Ok(qid) = passages.trim().parse::<u64>() else {
                return StatusCode::NOT_FOUND;
            };
            let delta = if action == "sooner" { -1 } else { 1 };
            ui.handle.send(Command::ShiftQueued(qid, delta));
            return StatusCode::NO_CONTENT;
        }
        _ => {}
    }
    let ids: Vec<i64> = passages.split(',').filter_map(|s| s.trim().parse().ok()).collect();
    if ids.is_empty() {
        return StatusCode::NOT_FOUND;
    }
    let place = match action.as_str() {
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
#[cfg(feature = "sampo-support")]
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
#[cfg(feature = "sampo-support")]
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
#[cfg(feature = "sampo-support")]
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

/// Serialises MusicBrainz search calls to roughly one per second
/// `[SPEC-SUI-196]` -- so opening two browser tabs and searching from both
/// still respects the API's limit, because the discipline lives in this one
/// process rather than in client behaviour a second tab simply would not
/// share. Same rate `tools/fetch_releases.py` already uses for the same API.
#[cfg(feature = "sampo-support")]
static MB_LAST_REQUEST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

#[cfg(feature = "sampo-support")]
async fn mb_rate_limit() {
    use std::time::{Duration, Instant};
    // The slot is reserved while holding the lock, and only the wait itself
    // happens after releasing it -- an `await` under a `std::sync::Mutex`
    // guard would hold it across a suspend point, which is the bug this
    // avoids rather than a style preference.
    let target = {
        let mut last = MB_LAST_REQUEST.lock().unwrap();
        let now = Instant::now();
        let earliest = last.map(|t| t + Duration::from_secs(1)).unwrap_or(now);
        let target = earliest.max(now);
        *last = Some(target);
        target
    };
    let now = Instant::now();
    if target > now {
        tokio::time::sleep(target - now).await;
    }
}

/// The same contact-bearing agent string `tools/fetch_releases.py` already
/// sends for the same API -- MusicBrainz asks for one and enforces it.
#[cfg(feature = "sampo-support")]
const MB_USER_AGENT: &str = "Vaino-Sampo/0.1 ( https://github.com/MangoCats/Vaino )";

#[cfg(feature = "sampo-support")]
#[derive(serde::Deserialize)]
struct MbSearchQuery {
    kind: String,
    q: String,
}

/// Search MusicBrainz directly `[SPEC-SUI-196]`, `[REQ-LIB-180]` -- for the
/// cases the fingerprint queue cannot reach: self-released audio with no
/// AcoustID entry, and a remaster or bootleg it has never indexed. The one
/// route the browser is allowed to reach musicbrainz.org through, so the rate
/// limit above cannot be bypassed by calling the API directly from the page.
///
/// Results come back shaped exactly like a fingerprint [`crate::db::Suggestion`]
/// so the page renders a searched match and a suggested one identically --
/// choosing either is the same action from the reviewer's side of the page.
#[cfg(feature = "sampo-support")]
async fn musicbrainz_search(
    axum::extract::Query(q): axum::extract::Query<MbSearchQuery>,
) -> axum::response::Response {
    if !matches!(q.kind.as_str(), "recording" | "artist" | "release") {
        return (StatusCode::BAD_REQUEST, "kind must be recording, artist or release")
            .into_response();
    }
    if q.q.trim().is_empty() {
        return axum::Json(Vec::<crate::db::Suggestion>::new()).into_response();
    }

    mb_rate_limit().await;

    let client = match reqwest::Client::builder().user_agent(MB_USER_AGENT).build() {
        Ok(c) => c,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let url = format!("https://musicbrainz.org/ws/2/{}", q.kind);
    let resp = client
        .get(&url)
        .query(&[("query", q.q.as_str()), ("fmt", "json"), ("limit", "15")])
        .send()
        .await;
    let body: serde_json::Value = match resp {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(v) => v,
            Err(_) => return StatusCode::BAD_GATEWAY.into_response(),
        },
        _ => return StatusCode::BAD_GATEWAY.into_response(),
    };

    axum::Json(parse_mb_results(&q.kind, &body)).into_response()
}

/// The parsing half of [`musicbrainz_search`], kept separate from the HTTP
/// call so it can be checked against a captured response without a network
/// -- MusicBrainz's own JSON shape differs per entity (`name` vs. `title`,
/// present or absent `artist-credit`), and that is exactly the part worth a
/// test's attention, not the fact that `reqwest` can fetch a URL.
#[cfg(feature = "sampo-support")]
fn parse_mb_results(kind: &str, body: &serde_json::Value) -> Vec<crate::db::Suggestion> {
    let key = match kind {
        "recording" => "recordings",
        "artist" => "artists",
        _ => "releases",
    };
    let is_artist = kind == "artist";
    body.get(key)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let mbid = item.get("id")?.as_str()?.to_string();
            let title = if is_artist { item.get("name") } else { item.get("title") }
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let artist = item
                .get("artist-credit")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|c| c.get("name"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let score = item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) / 100.0;
            Some(crate::db::Suggestion { mbid, title, artist, score })
        })
        .collect()
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

/// How long a skipped passage stays out of selection `[SPEC-PLAY-050]`.
async fn set_skip_suppress(
    State(ui): State<Ui>,
    axum::extract::Path(hours): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipSuppress(hours));
    StatusCode::NO_CONTENT
}

/// How long a passage removed from the queue unheard stays out
/// `[SPEC-PLAY-055]`.
async fn set_dequeue_suppress(
    State(ui): State<Ui>,
    axum::extract::Path(hours): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetDequeueSuppress(hours));
    StatusCode::NO_CONTENT
}

/// How many passages the Director keeps ahead `[SPEC-MPD-105]`.
async fn set_queue_depth(
    State(ui): State<Ui>,
    axum::extract::Path(n): axum::extract::Path<usize>,
) -> StatusCode {
    ui.handle.send(Command::SetQueueDepth(n));
    StatusCode::NO_CONTENT
}

/// How often a guest backend samples `status` `[SPEC-MPD-105]`.
async fn set_sample_interval(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSampleInterval(ms));
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
/// One passage's words `[SPEC-LYR-040]`.
///
/// **An endpoint rather than a snapshot field.** The snapshot is published on
/// every tick and read by every skin; up to 5.8 KB of text in it would be sent
/// hundreds of times to say what changes once a song. A skin fetches this when
/// the passage changes, which it already notices.
///
/// Plain text, because that is what the words are — a static block, as
/// MuLibPlay showed them `[SPEC-LYR-045]`. 404 means the library has none, which
/// is the ordinary case for 72% of passages and not an error worth dressing up.
async fn lyrics(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    let db = ui.db.clone();
    // The query blocks, so it belongs off the runtime.
    let found = tokio::task::spawn_blocking(move || {
        crate::db::Library::open(&db).ok().and_then(|lib| lib.lyrics(passage_id))
    })
    .await
    .ok()
    .flatten();
    match found {
        Some(text) => (
            [
                (axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                REVALIDATE,
            ],
            text,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

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

/// The editor's read side: what a passage's boundaries currently are
/// `[SPEC-SUI-201]`. A small JSON sibling of the editor page rather than data
/// baked into the page response -- every page Vaino serves is a static shell
/// compiled in with `include_str!`, and this one fetches its own state the
/// same way `/review` does `[SPEC021 §3]`.
#[cfg(feature = "sampo-support")]
async fn edit_info(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let found = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        let entry = lib.passage(passage_id).ok()?;
        // A recorded-but-not-yet-applied draft wins over the passage's own
        // values -- reopening the editor after a commit must show the edit
        // that was made, not the automatic values it drafted over
        // `[SPEC021 §2]`.
        let draft = lib.boundary_review(passage_id);
        Some((entry, draft))
    })
    .await
    .ok()
    .flatten();
    match found {
        Some((entry, draft)) => {
            let edited = draft.is_some();
            let (start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db) = match draft {
                Some(d) => (
                    d.start_ms,
                    d.end_ms,
                    d.lead_in_ms.unwrap_or(entry.lead_in_ms),
                    d.lead_out_ms.unwrap_or(entry.lead_out_ms),
                    d.gain_db.unwrap_or(entry.gain_db as f64),
                ),
                None => (
                    entry.start_ms,
                    entry.end_ms,
                    entry.lead_in_ms,
                    entry.lead_out_ms,
                    entry.gain_db as f64,
                ),
            };
            axum::Json(serde_json::json!({
                "passage_id": entry.passage_id,
                "start_ms": start_ms,
                "end_ms": end_ms,
                "file_ms": entry.file_ms,
                "lead_in_ms": lead_in_ms,
                "lead_out_ms": lead_out_ms,
                "gain_db": gain_db,
                "edited": edited,
            }))
            .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The five values a boundary edit posts `[SPEC021 §3]`, and nothing else --
/// no `passage_id`, which comes from the path and cannot be spoofed by the
/// body disagreeing with the URL.
#[cfg(feature = "sampo-support")]
#[derive(serde::Deserialize)]
struct BoundaryDraft {
    start_ms: u64,
    end_ms: u64,
    lead_in_ms: u64,
    lead_out_ms: u64,
    gain_db: f64,
}

/// Commit a boundary edit `[SPEC021 §2]`. Recorded, not applied -- the same
/// posture `id_reviews` takes and for the same reason: this changes what a
/// passage *is*, and the library is Sampo's to write.
#[cfg(feature = "sampo-support")]
async fn edit_review(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
    axum::extract::Json(draft): axum::extract::Json<BoundaryDraft>,
) -> axum::response::Response {
    let db = ui.db.clone();
    let done = tokio::task::spawn_blocking(move || {
        crate::db::PlayerStore::open(&db)
            .map_err(|e| e.message().to_string())?
            .record_boundary_review(
                passage_id,
                draft.start_ms,
                draft.end_ms,
                draft.lead_in_ms,
                draft.lead_out_ms,
                draft.gain_db,
            )
            .map_err(|e| e.message().to_string())
    })
    .await;
    match done {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(msg)) => (StatusCode::BAD_REQUEST, msg).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// The raw bytes of a passage's file, `Range`-aware `[SPEC021 §3]` -- the
/// file, not the passage. Vaino already resolves a passage to a file path for
/// playback; this reuses that resolution and streams bytes back rather than
/// decoding them, so `decodeAudioData` in the browser can fetch only the span
/// it needs instead of Vaino deciding that for it.
#[cfg(feature = "sampo-support")]
async fn edit_audio(
    State(ui): State<Ui>,
    axum::extract::Path(passage_id): axum::extract::Path<i64>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let db = ui.db.clone();
    let range = headers.get(axum::http::header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    // Both the path lookup and the file read block, so they belong off the
    // runtime -- the same reasoning `art_response` above already applies.
    let chunk = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open(&db).ok()?;
        let path = lib.passage_path(passage_id).ok()?;
        read_audio_range(&path, range.as_deref()).ok()
    })
    .await
    .ok()
    .flatten();

    match chunk {
        Some(c) if c.partial => (
            StatusCode::PARTIAL_CONTENT,
            [
                (axum::http::header::CONTENT_TYPE, c.content_type.to_string()),
                (axum::http::header::ACCEPT_RANGES, "bytes".to_string()),
                (axum::http::header::CONTENT_RANGE, format!("bytes {}-{}/{}", c.start, c.end, c.total)),
            ],
            c.bytes,
        )
            .into_response(),
        Some(c) => (
            StatusCode::OK,
            [
                (axum::http::header::CONTENT_TYPE, c.content_type.to_string()),
                (axum::http::header::ACCEPT_RANGES, "bytes".to_string()),
            ],
            c.bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// One slice of a file's bytes, and where it sits in the whole `[SPEC021 §3]`.
#[cfg(feature = "sampo-support")]
struct AudioChunk {
    bytes: Vec<u8>,
    start: u64,
    end: u64,
    total: u64,
    /// Whether this is less than the whole file -- decides `206` vs `200`.
    partial: bool,
    content_type: &'static str,
}

/// Read one byte range of `path`, or the whole file when `range` is absent,
/// unparseable, or a multi-range request -- the same "serve something useful
/// rather than refuse" choice most static file servers make, since a client
/// that cannot parse `Content-Range` back still gets a correct, if larger,
/// answer.
#[cfg(feature = "sampo-support")]
fn read_audio_range(path: &std::path::Path, range: Option<&str>) -> std::io::Result<AudioChunk> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let total = file.metadata()?.len();
    let content_type = mime_for_ext(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
    if total == 0 {
        return Ok(AudioChunk { bytes: Vec::new(), start: 0, end: 0, total: 0, partial: false, content_type });
    }
    let (start, end, partial) = match range.and_then(|r| parse_byte_range(r, total)) {
        Some((s, e)) => (s, e, true),
        None => (0, total - 1, false),
    };
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![0u8; (end - start + 1) as usize];
    file.read_exact(&mut bytes)?;
    Ok(AudioChunk { bytes, start, end, total, partial, content_type })
}

/// Parses one `Range: bytes=...` value against a known file length, per
/// [RFC 7233 §2.1](https://httpwg.org/specs/rfc7233.html#header.range):
/// `START-END`, `START-` (to the end), or `-SUFFIX` (the last SUFFIX bytes).
/// A second range after a comma is ignored rather than honoured -- multi-range
/// responses are a different wire format (`multipart/byteranges`) this route
/// does not speak, and pretending to would be worse than not trying.
#[cfg(feature = "sampo-support")]
fn parse_byte_range(header: &str, total: u64) -> Option<(u64, u64)> {
    let spec = header.strip_prefix("bytes=")?.split(',').next()?.trim();
    let (start_s, end_s) = spec.split_once('-')?;
    let last = total - 1;
    let (start, end) = if start_s.is_empty() {
        let suffix: u64 = end_s.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        (last.saturating_sub(suffix - 1), last)
    } else {
        let start: u64 = start_s.parse().ok()?;
        let end = if end_s.is_empty() { last } else { end_s.parse::<u64>().ok()?.min(last) };
        (start, end)
    };
    (start <= end && start <= last).then_some((start, end))
}

/// The `Content-Type` for a file extension Vaino might be asked to stream
/// raw, so the browser's `<audio>`/`decodeAudioData` picks the right decoder
/// instead of guessing from bytes. Unknown extensions still serve -- the
/// bytes are correct either way, just without a hint.
#[cfg(feature = "sampo-support")]
fn mime_for_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "flac" => "audio/flac",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "m4a" | "aac" => "audio/mp4",
        "wma" => "audio/x-ms-wma",
        _ => "application/octet-stream",
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
/// Make imported music selectable without restarting the player
/// `[IMPL-SUI-075]`.
///
/// Browse reads the library live and sees an import at once; the Program
/// Director does not, because it is loaded once at startup and nothing reloaded
/// it. This asks for a rebuild — it does not perform one. The engine picks the
/// intent up on its next refill and starts only when the queue can afford it,
/// which is why the reply is *accepted* rather than *done*.
async fn reload_library(State(ui): State<Ui>) -> StatusCode {
    let Ok(mut c) = ui.controls.lock() else { return StatusCode::INTERNAL_SERVER_ERROR };
    c.reload_requested = true;
    c.reload_status = Some("requested".into());
    StatusCode::ACCEPTED
}

/// Allow or forbid Vaino writing cue sheets into the music folder
/// `[REQ-VIS-205]`.
///
/// The four settings that let Vaino write files outside its own storage.
///
/// **Written as one macro so that changing one is changing all four.** They are
/// the same handler with a different flag: take `on`/`off`, tell the engine so
/// the choice persists, and leave an intent for the loop to act on — because
/// acting means walking the library and writing into a folder Vaino does not
/// own, which is not work for a request handler to do while a browser waits.
///
/// The generation each one triggers is the matching table in `vaino.rs`; the two
/// lists are the same four in the same order, and neither is complete without
/// the other. Adding a fifth means an arm here, an entry there, a column of
/// none — settings are rows now `[SPEC-SC-099]` — and a checkbox in the skin.
macro_rules! writes_files {
    ($($fn_name:ident => $cmd:ident, $asked:ident, $status:ident, $what:literal, $req:literal;)+) => {
        $(
            #[doc = concat!("Allow or forbid Vaino writing ", $what, " `", $req, "`.")]
            ///
            /// One of four; see [`writes_files`].
            async fn $fn_name(
                State(ui): State<Ui>,
                axum::extract::Path(on): axum::extract::Path<String>,
            ) -> StatusCode {
                let want = on == "on" || on == "true" || on == "1";
                ui.handle.send(Command::$cmd(want));
                let Ok(mut c) = ui.controls.lock() else {
                    return StatusCode::INTERNAL_SERVER_ERROR;
                };
                c.$asked = Some(want);
                c.$status = Some(if want { "writing…".into() } else { "off".into() });
                StatusCode::ACCEPTED
            }
        )+
    };
}

writes_files! {    set_cue_sheets => SetCueSheets, cue_requested, cue_status,
        "cue sheets into the music folder", "[REQ-VIS-205]";
    set_covers => SetCovers, covers_requested, covers_status,
        "cover art into the music folder", "[REQ-VIS-210]";
    set_lyrics_cache => SetLyricsCache, lyrics_requested, lyrics_status,
        "per-song lyrics into a local client's cache", "[REQ-VIS-215]";
    set_lyrics_sidecar => SetLyricsSidecar, sidecar_requested, sidecar_status,
        "lyrics beside the audio", "[REQ-VIS-220]";
}

/// Ask for the other backend `[SPEC-BK-030]`.
///
/// Asks; it does not perform. The engine takes the intent on its next pass,
/// where the backends actually live — the same reason `reload_library` is a
/// request. The reply is *accepted*, and what the switch managed to carry
/// appears in `switch_status` a moment later `[SPEC-BK-045]`.
async fn switch_backend(
    State(ui): State<Ui>,
    axum::extract::Path(which): axum::extract::Path<String>,
) -> StatusCode {
    if which != "vaino" && which != "mpd" {
        return StatusCode::BAD_REQUEST;
    }
    let Ok(mut c) = ui.controls.lock() else { return StatusCode::INTERNAL_SERVER_ERROR };
    if !c.guest_available {
        return StatusCode::CONFLICT; // nothing to switch to
    }
    c.switch_requested = Some(which);
    c.switch_status = Some("requested".into());
    StatusCode::ACCEPTED
}

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
            snap.reload_status = c.reload_status.clone();
            snap.backend = c.backend.clone();
            snap.guest_available = c.guest_available;
            snap.guest_name = c.guest_name.clone();
            snap.switch_status = c.switch_status.clone();
            snap.cue_status = c.cue_status.clone();
            snap.covers_status = c.covers_status.clone();
            snap.lyrics_status = c.lyrics_status.clone();
            snap.can_seek = c.can_seek;
            snap.sidecar_status = c.sidecar_status.clone();
            snap.pool = c.pool;
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

/// Restart the player, without restarting the machine `[REQ-VIS-250]`.
///
/// **Several settings only take effect at startup**, because what they change
/// is read once: cue tracks are mapped when the guest is attached
/// `[REQ-VIS-205]`, and the Director builds its pool from the library as it
/// stands. Telling a listener to "restart the player" on an appliance whose
/// only interface is this page previously meant an SSH session or the plug.
///
/// The same first step as a shutdown, and for the same reason: the resume
/// point is otherwise written on an interval `[REQ-VIS-155]`, so a deliberate
/// restart would still lose up to that much position — in exactly the case
/// somebody took care over.
///
/// **202, not 204.** The service is about to be stopped by the thing it is
/// asking, so this reply cannot honestly claim the restart finished
/// `[PI3-API-030]`.
async fn restart_player(State(ui): State<Ui>) -> Response {
    ui.handle.send(Command::Persist);
    tokio::time::sleep(Duration::from_millis(400)).await;

    // `restart` rather than `stop` then `start`: systemd owns the ordering,
    // and this process does not survive to run a second command anyway.
    match std::process::Command::new("sudo")
        .arg("-n")
        .arg("systemctl")
        .arg("restart")
        .arg("vaino")
        .spawn()
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            "restarting; the page will reconnect on its own in a few seconds",
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not restart: {e}"),
        )
            .into_response(),
    }
}

/// Shut the appliance down gracefully `[PI5-PWR-010]`.
///
/// An appliance whose only interface is a web page has no other way to be
/// turned off, and pulling its power is how an SD card is corrupted and how a
/// database is left mid-write. This exists so that stops being the only option.
///
/// Three steps, in order, and the first is the one a bare `poweroff` misses:
///
/// 1. **Write the resume point now.** It is otherwise saved on an interval
///    `[REQ-VIS-155]`, so a deliberate shutdown would still lose up to that
///    much position -- in exactly the case someone took care over.
/// 2. **Hand off to systemd**, which stops the services and unmounts, rather
///    than cutting power under a live filesystem.
/// 3. **Answer 202, not 204.** The request is accepted; whether the machine
///    completes it is not something this reply can honestly claim, since the
///    process making it is about to be stopped.
async fn power_off(State(ui): State<Ui>) -> Response {
    ui.handle.send(Command::Persist);
    // Long enough for the engine to take the command off the channel and write
    // one row; short enough that a person does not wonder if the click landed.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Detached, and deliberately not awaited: systemd stops this very service
    // as part of the transition, so waiting for the child to exit would mean
    // waiting to be killed.
    match std::process::Command::new("sudo")
        .arg("-n")
        .arg("systemctl")
        .arg("poweroff")
        .spawn()
    {
        Ok(_) => (
            StatusCode::ACCEPTED,
            "shutting down; wait for the light to go out before pulling power",
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("could not begin shutdown: {e}"),
        )
            .into_response(),
    }
}

/// Every radio and whether it is blocked `[PI3-RF-010]`.
///
/// Built because a blocked radio is indistinguishable, from the settings page,
/// from a broken button: the Middleton was paired, bonded, trusted and
/// advertising, and Connect did nothing at all because `hci0` was soft-blocked
/// `[PI3-FOUND-050]`. One line saying so would have ended that evening.
async fn radios() -> Response {
    bt_reply(bluetooth::run(bluetooth::Verb::Radios, None), false)
}

/// Switch one radio on or off `[PI3-RF-020]`.
///
/// The helper refuses to block whichever radio carries the default route, and
/// this deliberately does **not** repeat that rule -- one copy, on the side
/// that holds the privilege, so a second caller cannot be told something
/// different `[PI3-RF-030]`.
async fn set_radio(
    axum::extract::Path((kind, state)): axum::extract::Path<(String, String)>,
) -> Response {
    let on = match state.as_str() {
        "on" => true,
        "off" => false,
        _ => return (StatusCode::NOT_FOUND, "state is on or off").into_response(),
    };
    bt_reply(bluetooth::set_radio(&kind, on), false)
}

async fn speakers() -> Response {
    bt_reply(bluetooth::run(bluetooth::Verb::List, None), false)
}

/// Verbs that name no device: `scan`.
async fn speaker_verb(
    State(ui): State<Ui>,
    axum::extract::Path(verb): axum::extract::Path<String>,
) -> Response {
    let Some(v) = bluetooth::Verb::parse(&verb) else {
        return (StatusCode::NOT_FOUND, "unknown verb").into_response();
    };
    if v.needs_address() {
        return (StatusCode::BAD_REQUEST, "verb needs a device").into_response();
    }
    let _ = &ui;
    bt_reply(bluetooth::run(v, None), false)
}

/// Verbs that name a device. `use` reopens the player output as part of the
/// same request `[PI3-UI-020]`: the stream does not dependably follow a change
/// of default sink, so a selection that stopped at the helper would look like
/// it worked and be silent. Doing it here rather than in the browser means no
/// caller can forget the step that makes the choice audible.
async fn speaker_verb_on(
    State(ui): State<Ui>,
    axum::extract::Path((verb, address)): axum::extract::Path<(String, String)>,
) -> Response {
    let Some(v) = bluetooth::Verb::parse(&verb) else {
        return (StatusCode::NOT_FOUND, "unknown verb").into_response();
    };
    let result = bluetooth::run(v, Some(&address));
    let reopen = result.is_ok() && matches!(v, bluetooth::Verb::Use | bluetooth::Verb::Pair);
    if reopen {
        // Remembered so the appliance's own reconnect timer knows which
        // absent device is worth paging `[PI3-AIM-020]`, `[REQ-VIS-260]` --
        // best-effort, since a listener whose speaker just started working
        // should not be told it failed over a bookkeeping write.
        let db = ui.db.clone();
        let _ = tokio::task::spawn_blocking(move || match crate::db::PlayerStore::open(&db) {
            Ok(store) => {
                if let Err(e) = store.save_speaker_address(&address) {
                    eprintln!("save speaker address: {e}");
                }
            }
            Err(e) => eprintln!("save speaker address: {e}"),
        })
        .await;
        ui.handle.send(Command::ReopenOutput);
        // Wait for the reopen to land before reporting where the audio went.
        // Reading the sink immediately gives sink:null with dummy:false, which
        // reads as healthy and is merely early -- the precise shape of
        // reassuring answer that hid this fault in the first place.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
    }
    bt_reply(result, reopen)
}

/// One shape for every speaker reply, so the panel has one thing to read.
fn bt_reply(result: Result<serde_json::Value, String>, reopened: bool) -> Response {
    match result {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("reopened".into(), reopened.into());
                // The answer the listener actually cares about, and the one
                // the helper cannot give: where the audio ended up
                // `[PI3-API-020]`.
                let where_to = crate::sink::current();
                // An unlinked stream is not a working one. Saying so keeps
                // "we could not tell" distinct from "it is fine".
                if where_to.known && where_to.sink.is_none() {
                    obj.insert("audible".into(), serde_json::Value::Null);
                } else {
                    obj.insert("audible".into(), (!where_to.dummy).into());
                }
                if let Ok(s) = serde_json::to_value(where_to) {
                    obj.insert("output".into(), s);
                }
            }
            axum::Json(v).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
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
#[cfg(feature = "sampo-support")]
const REVIEW_HTML: &str = include_str!("web/review.html");
#[cfg(feature = "sampo-support")]
const REVIEW_JS: &str = include_str!("web/review.js");
#[cfg(feature = "sampo-support")]
const EDIT_HTML: &str = include_str!("web/edit.html");
#[cfg(feature = "sampo-support")]
const EDIT_JS: &str = include_str!("web/edit.js");
/// The exponential ramp curve, ported from `fade.rs` and checked against the
/// same fixture `[SPEC021 §4]`. Its own file and route so the editor's
/// preview and `fade.rs`'s real playback provably agree, not just resemble
/// each other.
#[cfg(feature = "sampo-support")]
const FADE_JS: &str = include_str!("web/fade.js");

/// The route the review page fetches its work from, named once so the router
/// and the test that checks the page agrees with it cannot drift apart.
#[cfg(feature = "sampo-support")]
const REVIEW_QUEUE_ROUTE: &str = "/review/queue";
/// The prefix every decision is posted to. The page builds the rest of the
/// path from the passage id, so only the stem can be shared.
#[cfg(all(test, feature = "sampo-support"))]
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
        qid: 0, // stamped by Queue on the way in
            passage_id: id,
            path: PathBuf::from(format!("/music/{title}.mp3")),
            start_ms: 0,
            end_ms: 200_000,
            file_ms: 0,
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
        // A const array, so this is checked when it compiles rather than here;
        // the assertion below is the one that can actually fail.
        const _: () = assert!(!SKINS.is_empty());
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
        // Reviewing ids is reachable, or it may as well not exist -- it is not
        // linked from the player, deliberately, so browse is the only way in.
        // The literal link is compiled into `BROWSE_HTML` regardless of
        // whether `/review` itself is served this build `[SPEC-SUI-190]`, so
        // this assertion holds either way; the route's own existence is
        // `sampo-support`'s to check.
        assert!(BROWSE_HTML.contains("/review"), "no way to reach the review page");
    }

    /// The review page loads core the same way the others do -- gated with
    /// everything else it depends on `[SPEC-SUI-190]`.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn the_review_page_loads_the_runtime() {
        assert!(REVIEW_HTML.contains("/core.js") && REVIEW_HTML.contains("/review.js"));
        assert!(REVIEW_JS.contains("startBare"), "review takes the skin, not the player");
    }

    /// The review page and its routes have to agree about the URLs, which is
    /// the seam a jsdom check cannot see: it mocks `fetch`, so a page asking
    /// for a route the server never registered still passes there.
    #[cfg(feature = "sampo-support")]
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

    /// The editor page and its own JS, present only behind the feature that
    /// gates every other Sampo-support page `[SPEC-SUI-190]`.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn the_edit_page_loads_the_runtime() {
        assert!(EDIT_HTML.contains("/core.js") && EDIT_HTML.contains("/edit.js"));
        assert!(EDIT_JS.contains("startBare"), "edit takes the skin, not the player");
    }

    /// The page and the router have to agree about the URLs it fetches --
    /// the seam a jsdom check of the JS alone cannot see `[SPEC021 §3]`.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn the_edit_page_asks_for_routes_the_router_serves() {
        assert!(EDIT_JS.contains("/edit/${passageId}/info"));
        assert!(EDIT_JS.contains("/edit/${passageId}/audio"));
        assert!(EDIT_JS.contains("/edit/${passageId}/review"));
    }

    /// The wire shape `edit_review` accepts must be exactly what the store
    /// can write -- an extra or renamed field here is invisible to `cargo
    /// check` because `serde` just ignores unknown fields by default.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn boundary_draft_deserialises_the_five_values_the_store_wants() {
        let draft: BoundaryDraft = serde_json::from_str(
            r#"{"start_ms":1000,"end_ms":2000,"lead_in_ms":50,"lead_out_ms":900,"gain_db":-1.5}"#,
        )
        .unwrap();
        assert_eq!((draft.start_ms, draft.end_ms), (1000, 2000));
        assert_eq!((draft.lead_in_ms, draft.lead_out_ms), (50, 900));
        assert!((draft.gain_db - -1.5).abs() < 1e-9);
    }

    /// `bytes=START-END`, `bytes=START-` and `bytes=-SUFFIX` per RFC 7233 --
    /// the three forms a real browser actually sends, plus the malformed and
    /// out-of-bounds cases that must fall back to "serve the whole file"
    /// rather than panic or serve garbage.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn byte_ranges_parse_the_way_a_browser_sends_them() {
        assert_eq!(parse_byte_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_byte_range("bytes=500-", 1000), Some((500, 999)));
        assert_eq!(parse_byte_range("bytes=-100", 1000), Some((900, 999)));
        // Past the end of the file clamps rather than overruns.
        assert_eq!(parse_byte_range("bytes=900-99999", 1000), Some((900, 999)));
        // A second range is ignored, not honoured -- this route does not
        // speak `multipart/byteranges`.
        assert_eq!(parse_byte_range("bytes=0-9,20-29", 1000), Some((0, 9)));
        // Malformed or inverted inputs report "could not parse" so the
        // caller serves the whole file instead of guessing.
        assert_eq!(parse_byte_range("nonsense", 1000), None);
        assert_eq!(parse_byte_range("bytes=", 1000), None);
        assert_eq!(parse_byte_range("bytes=-0", 1000), None);
        assert_eq!(parse_byte_range("bytes=500-100", 1000), None);
        assert_eq!(parse_byte_range("bytes=5000-", 1000), None);
    }

    /// A real file on disk, read whole and read in a slice -- the seam
    /// `parse_byte_range`'s unit tests cannot reach, since it never touches
    /// the filesystem.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn read_audio_range_serves_the_whole_file_or_the_asked_for_slice() {
        let path = std::env::temp_dir().join(format!("vaino-edit-audio-{}.bin", std::process::id()));
        std::fs::write(&path, b"0123456789").unwrap();

        let whole = read_audio_range(&path, None).unwrap();
        assert_eq!(whole.bytes, b"0123456789");
        assert!(!whole.partial);
        assert_eq!(whole.total, 10);

        let slice = read_audio_range(&path, Some("bytes=2-4")).unwrap();
        assert_eq!(slice.bytes, b"234");
        assert!(slice.partial);
        assert_eq!((slice.start, slice.end, slice.total), (2, 4, 10));

        // A range this route cannot parse falls back to the whole file rather
        // than an error -- a client that sent it still gets a correct answer.
        let garbled = read_audio_range(&path, Some("garbage")).unwrap();
        assert!(!garbled.partial);
        assert_eq!(garbled.bytes, b"0123456789");

        std::fs::remove_file(&path).ok();
    }

    /// A recording search response, in MusicBrainz's own shape -- `title`,
    /// `artist-credit[0].name`, `score` 0-100 `[SPEC-SUI-196]`.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn mb_recording_results_parse_into_suggestions() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"recordings":[{"id":"rec-1","title":"Why Worry","score":97,
                 "artist-credit":[{"name":"Dire Straits"}]},
                {"id":"rec-2","title":"No Artist Credit","score":80}]}"#,
        )
        .unwrap();
        let out = parse_mb_results("recording", &body);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].mbid, "rec-1");
        assert_eq!(out[0].title, Some("Why Worry".into()));
        assert_eq!(out[0].artist, Some("Dire Straits".into()));
        assert!((out[0].score - 0.97).abs() < 1e-9, "score must be normalised to 0..1");
        assert_eq!(out[1].artist, None, "no artist-credit must not panic or fabricate one");
    }

    /// An artist search names itself with `name`, not `title` -- the one
    /// place MusicBrainz's own shape actually differs by entity kind.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn mb_artist_results_read_name_not_title() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"artists":[{"id":"art-1","name":"Dire Straits","score":100}]}"#,
        )
        .unwrap();
        let out = parse_mb_results("artist", &body);
        assert_eq!(out[0].title, Some("Dire Straits".into()));
    }

    /// No results, or a shape this parser was not given a key for, must come
    /// back as an empty list rather than panicking on the request that
    /// probably matters most: a search for something that genuinely is not
    /// on MusicBrainz at all.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn mb_results_with_nothing_found_is_an_empty_list_not_a_panic() {
        let body: serde_json::Value = serde_json::from_str(r#"{"recordings":[]}"#).unwrap();
        assert!(parse_mb_results("recording", &body).is_empty());
        let empty = serde_json::json!({});
        assert!(parse_mb_results("release", &empty).is_empty());
    }

    /// The review page's own search box has to agree with the router about
    /// this route, the same seam every other page-to-route check here guards.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn the_review_page_asks_musicbrainz_search_for_the_route_it_gets() {
        assert!(REVIEW_JS.contains("/api/musicbrainz/search"));
    }

    #[cfg(feature = "sampo-support")]
    #[test]
    fn mime_for_ext_knows_the_formats_this_library_actually_has() {
        assert_eq!(mime_for_ext("flac"), "audio/flac");
        assert_eq!(mime_for_ext("MP3"), "audio/mpeg");
        assert_eq!(mime_for_ext("wav"), "audio/wav");
        assert_eq!(mime_for_ext("xyz"), "application/octet-stream");
    }
}
