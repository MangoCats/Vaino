//! Every plain "set this number/flag and let the engine pick it up"
//! handler `[SPEC-SC-099]`, including the four folder-writing toggles
//! generated together so changing one is changing all four.

use axum::extract::State;
use axum::http::StatusCode;

use crate::engine::Command;

use super::Ui;

/// How often the resume point is written `[REQ-VIS-155]`.
pub(super) async fn set_resume_save(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetResumeSave(ms));
    StatusCode::NO_CONTENT
}

/// How long a skipped passage stays out of selection `[SPEC-PLAY-050]`.
pub(super) async fn set_skip_suppress(
    State(ui): State<Ui>,
    axum::extract::Path(hours): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipSuppress(hours));
    StatusCode::NO_CONTENT
}

/// How long a passage removed from the queue unheard stays out
/// `[SPEC-PLAY-055]`.
pub(super) async fn set_dequeue_suppress(
    State(ui): State<Ui>,
    axum::extract::Path(hours): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetDequeueSuppress(hours));
    StatusCode::NO_CONTENT
}

/// How many passages the Director keeps ahead `[SPEC-MPD-105]`.
pub(super) async fn set_queue_depth(
    State(ui): State<Ui>,
    axum::extract::Path(n): axum::extract::Path<usize>,
) -> StatusCode {
    ui.handle.send(Command::SetQueueDepth(n));
    StatusCode::NO_CONTENT
}

/// How often a guest backend samples `status` `[SPEC-MPD-105]`.
pub(super) async fn set_sample_interval(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSampleInterval(ms));
    StatusCode::NO_CONTENT
}

/// How long a skip fades the outgoing passage out, in ms. Clamped by the
/// engine, which owns the limits `[REQ-AUD-162]`.
pub(super) async fn set_skip_fade(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipFade(ms));
    StatusCode::NO_CONTENT
}

/// How long after a skip the next passage starts, in ms `[REQ-AUD-162]`.
pub(super) async fn set_skip_lead(
    State(ui): State<Ui>,
    axum::extract::Path(ms): axum::extract::Path<u64>,
) -> StatusCode {
    ui.handle.send(Command::SetSkipLead(ms));
    StatusCode::NO_CONTENT
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
            pub(super) async fn $fn_name(
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

