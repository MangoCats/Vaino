//! What a session needs of whatever is playing `[GDE-EXT-055]`.
//!
//! **A spike, on `investigate/external-players`.** It exists to measure the
//! cost of driving something other than the built-in engine — MPD, an
//! OpenSubsonic server — rather than to ship that. The measurement it makes is
//! the width of this file: if the trait is small, the Director is portable; if
//! it is wide, it is not.
//!
//! The seam was already here and had not been named. Measured on this tree:
//!
//! * `Session` calls **seven** methods on `Engine`, and a binary's loop three
//!   more.
//! * The **Director touches `path` nowhere** — the only occurrences under
//!   `director/` are a comment and a test fixture's DDL.
//! * Exactly **one line** turns a queue entry into audio:
//!   `PassageDecoder::open(&e.path, …)` in `engine.rs`. Every other use of
//!   `entry.path` is an error message, a filename for display, or `relink`,
//!   which is a different subsystem entirely.
//!
//! So selection is already independent of playback. What follows only writes
//! that down.

use crate::queue::QueueEntry;

/// What a backend can actually do, declared rather than assumed.
///
/// The temptation is a trait that pretends every backend is equivalent, and it
/// must be refused. Vaino selects a **span** — `start_ms` to `end_ms`, with
/// lead-in, lead-out and gain `[SPEC-SC-040]` — and MPD and OpenSubsonic
/// address whole files. A backend that cannot honour a span will play the whole
/// file instead, which for a DAO capture means forty songs where one was
/// chosen `[GDE-EXT-025]`.
///
/// Reporting that is the whole point. `[PI3-API-030]` is the standing precedent:
/// an output that accepts audio perfectly and is inaudible is a player lying
/// about what it is doing, and this project treats that as the fault to remove
/// rather than the cost of doing business. A backend that silently plays the
/// wrong forty minutes is the same fault wearing a network protocol.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Capabilities {
    /// Can it start and stop inside a file? False for MPD and OpenSubsonic as
    /// commonly deployed, and the reason `kind='radio'` trim points do not
    /// survive the trip.
    pub spans: bool,
    /// Can it apply per-passage gain `[SPEC-SC-040]`?
    pub gain: bool,
    /// Can it ramp in and out, or does a passage begin and end abruptly?
    pub ramps: bool,
}

impl Capabilities {
    /// Everything, which is what the built-in engine does.
    pub const FULL: Self = Self { spans: true, gain: true, ramps: true };
    /// A whole-file backend: it plays what you name, start to end.
    ///
    /// OpenSubsonic as deployed. `stream` returns a song; HTTP range requests
    /// seek within the bytes, which is not the same as being told to stop.
    pub const WHOLE_FILE: Self = Self { spans: false, gain: false, ramps: false };

    /// **MPD, and the surprise of this investigation.**
    ///
    /// `rangeid {ID} {START:END}` has specified the portion of a song to play
    /// since MPD 0.19, in fractional seconds, with either end optional. That is
    /// Vaino's passage model almost exactly, so the Album/Radio duality
    /// `[GDE-BMK-030]` survives the trip to MPD — where it does not survive the
    /// trip to OpenSubsonic. `crossfade` exists but is global rather than
    /// per-passage, and ReplayGain is per-file rather than per-span, so gain
    /// and ramps are still lost.
    pub const MPD: Self = Self { spans: true, gain: false, ramps: false };

    /// Would sending this passage to this backend play something other than
    /// what was chosen?
    ///
    /// True only when the passage is a genuine slice. A passage covering its
    /// whole file — which is every `ingest:whole-file` row, and most of a
    /// single-track library — is unharmed by a backend that cannot clip.
    pub fn would_misplay(&self, entry: &QueueEntry, file_duration_ms: u64) -> bool {
        if self.spans {
            return false;
        }
        let covers_whole = entry.start_ms == 0
            && entry.end_ms + WHOLE_FILE_SLACK_MS >= file_duration_ms;
        !covers_whole
    }
}

/// Within this much of the end, a passage is the whole file. The same slack
/// `extract_library.py` uses when deciding whether to slice before extracting.
pub const WHOLE_FILE_SLACK_MS: u64 = 5_000;

/// The whole of what a session asks of a player.
///
/// Ten methods, of which the last three are lifecycle. `Engine` satisfies it
/// today without modification, which is the finding: no local behaviour has to
/// change for a second backend to become possible.
pub trait Playback {
    fn capabilities(&self) -> Capabilities;

    /// Add a passage to the back of the queue.
    fn enqueue(&mut self, entry: QueueEntry);

    /// What is queued, in play order. The Director reads this to avoid
    /// choosing something already coming `[SPEC-DIR-160]`.
    fn queued(&self) -> Vec<QueueEntry>;

    /// How many more passages are wanted to reach the configured depth.
    fn shortfall(&self) -> usize;

    /// Passages the backend could not open, so the Director can un-count them
    /// `[REQ-PD-112]`. A remote backend reports the same thing for a song the
    /// server has forgotten.
    fn take_dropped(&mut self) -> Vec<i64>;

    /// Do a slice of work. For the local engine this mixes; for a remote
    /// backend it polls the server and reconciles.
    fn tick(&mut self) -> usize;

    fn is_shutdown(&self) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built out rather than defaulted: `QueueEntry` has no `Default`, and a
    /// spike is not a reason to add one to a production type.
    fn entry(start: u64, end: u64) -> QueueEntry {
        QueueEntry {
            qid: 0,
            passage_id: 1,
            path: std::path::PathBuf::from("/m/a.mp3"),
            start_ms: start,
            end_ms: end,
            lead_in_ms: 0,
            lead_out_ms: 0,
            gain_db: 0.0,
            mbid: None,
            naming: Default::default(),
        }
    }

    #[test]
    fn a_full_backend_never_misplays() {
        assert!(!Capabilities::FULL.would_misplay(&entry(30_000, 240_000), 600_000));
    }

    #[test]
    fn a_whole_file_backend_is_fine_with_whole_file_passages() {
        // The `ingest:whole-file` case, and most of a single-track library.
        assert!(!Capabilities::WHOLE_FILE.would_misplay(&entry(0, 284_250), 284_250));
    }

    #[test]
    fn a_whole_file_backend_would_misplay_a_dao_slice() {
        // Track 7 of a 40-track capture: naming the file plays the whole hour.
        assert!(Capabilities::WHOLE_FILE.would_misplay(&entry(1_200_000, 1_440_000), 8_000_000));
    }

    #[test]
    fn mpd_keeps_the_dao_slice_that_subsonic_loses() {
        // The same passage, to two backends. Measured on the real library:
        // 98.6% of radio passages are trimmed slices, so this is the common
        // case and not an edge one.
        let dao = entry(1_200_000, 1_440_000);
        assert!(!Capabilities::MPD.would_misplay(&dao, 8_000_000));
        assert!(Capabilities::WHOLE_FILE.would_misplay(&dao, 8_000_000));
    }

    #[test]
    fn trailing_slack_still_counts_as_whole() {
        // A trim that stops a few seconds early is not a slice worth refusing.
        assert!(!Capabilities::WHOLE_FILE.would_misplay(&entry(0, 280_000), 284_250));
    }
}

/// The built-in engine, satisfying the trait it did not know about.
///
/// **The finding this spike exists for.** Every method below forwards to one
/// that already existed with the same signature: nothing in `engine.rs` was
/// touched to make this compile. The seam was real before it was named, which
/// is why a second backend is an addition rather than a refactor.
impl Playback for crate::engine::Engine {
    fn capabilities(&self) -> Capabilities {
        Capabilities::FULL
    }
    fn enqueue(&mut self, entry: QueueEntry) {
        crate::engine::Engine::enqueue(self, entry)
    }
    fn queued(&self) -> Vec<QueueEntry> {
        crate::engine::Engine::queued(self).cloned().collect()
    }
    fn shortfall(&self) -> usize {
        crate::engine::Engine::shortfall(self)
    }
    fn take_dropped(&mut self) -> Vec<i64> {
        crate::engine::Engine::take_dropped(self)
    }
    fn tick(&mut self) -> usize {
        crate::engine::Engine::tick(self)
    }
    fn is_shutdown(&self) -> bool {
        crate::engine::Engine::is_shutdown(self)
    }
}
