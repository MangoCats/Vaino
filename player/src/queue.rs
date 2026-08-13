//! Playback queue and crossfade timing.
//!
//! SPEC002 describes crossfade admission as three cases with three diagrams
//! `[XFD-BEH-C1-020]`, `[XFD-BEH-C2-020]`, `[XFD-BEH-C3-*]`. They are one rule:
//!
//! ```text
//!     overlap = min(lead_out(A), lead_in(B))
//! ```
//!
//! - Case 1, `lead_out(A) <= lead_in(B)`: B starts at A's lead-out point, so the
//!   overlap is `lead_out(A)` — the smaller.
//! - Case 2, `lead_out(A) > lead_in(B)`: B starts when A has `lead_in(B)`
//!   remaining, so the overlap is `lead_in(B)` — again the smaller.
//! - Case 3, either duration zero: no overlap, which `min` already gives.
//!
//! Implementing the three cases separately would mean three code paths that can
//! disagree, for a rule that is one `min`.

use std::collections::VecDeque;
use std::path::PathBuf;

/// A passage waiting to play. Timing only — no audio, no decoder.
///
/// **Lead durations are usually milliseconds, and that is correct.** Measured
/// across the migrated library: lead-in median 5 ms, lead-out median 946 ms.
/// That looks like missing data but is the preferred configuration — the ramps
/// exist mainly to hide the short, sometimes loud pops at a track's start and
/// end, which takes only a few milliseconds. Audible crossfade is the rare
/// case, wanted where a track genuinely fades out slowly and the alternative
/// would be a long near-silent gap.
///
/// So `overlap_ms` yielding ~0 for most pairs is the intended outcome, not a
/// fault to be "fixed" by inflating the leads.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntry {
    pub passage_id: i64,
    pub path: PathBuf,
    pub start_ms: u64,
    pub end_ms: u64,
    /// Silence-to-full at the start; 0 means begin at full level.
    pub lead_in_ms: u64,
    /// Full-to-silence at the end; 0 means end abruptly.
    pub lead_out_ms: u64,
    pub gain_db: f32,
    /// The recording this passage is, for play history `[SPEC-SC-095]`.
    /// `None` when unidentified — such a passage still plays, it simply
    /// cannot contribute to rotation.
    pub mbid: Option<String>,
}

impl QueueEntry {
    pub fn duration_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// A human label for this passage.
    ///
    /// The filename stem, until Sampo supplies real titles `[SPEC-SA-100]`.
    /// Here rather than in each display path so the browser, the decision
    /// record and the terminal all name a passage the same way — and so the
    /// filesystem path stays inside the process.
    pub fn title(&self) -> String {
        self.path.file_stem().unwrap_or_default().to_string_lossy().to_string()
    }
}

/// Overlap between two consecutive passages — the whole of SPEC002's timing.
///
/// Clamped to the shorter passage so a lead longer than the audio cannot ask
/// for an overlap that does not exist.
pub fn overlap_ms(a: &QueueEntry, b: &QueueEntry) -> u64 {
    a.lead_out_ms
        .min(b.lead_in_ms)
        .min(a.duration_ms())
        .min(b.duration_ms())
}

/// Should `next` start now, given how far `current` has played?
///
/// Position-driven, never buffer-driven: keying admission off buffer occupancy
/// makes the overlap depend on how fast the consumer happens to drain, which
/// silently vanishes without back-pressure and varies under load.
pub fn should_admit(current: &QueueEntry, played_ms: u64, next: &QueueEntry) -> bool {
    let remaining = current.duration_ms().saturating_sub(played_ms);
    remaining <= overlap_ms(current, next)
}

/// The upcoming passages, in order.
///
/// Holds no audio and no decoders: this is scheduling, and keeping it that way
/// is what lets the timing rules be tested without touching a file.
#[derive(Debug, Default)]
pub struct Queue {
    entries: VecDeque<QueueEntry>,
    /// Below this depth the Program Director is asked for more `[REQ-PD-100]`.
    pub min_depth: usize,
}

impl Queue {
    pub fn new(min_depth: usize) -> Self {
        Self { entries: VecDeque::new(), min_depth }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn peek(&self) -> Option<&QueueEntry> {
        self.entries.front()
    }
    pub fn peek_next(&self) -> Option<&QueueEntry> {
        self.entries.get(1)
    }

    /// Append, as the Program Director's selections arrive.
    pub fn push(&mut self, e: QueueEntry) {
        self.entries.push_back(e);
    }

    /// Insert immediately after the playing passage — a user "play next".
    pub fn push_after_current(&mut self, e: QueueEntry) {
        let at = if self.entries.is_empty() { 0 } else { 1 };
        self.entries.insert(at, e);
    }

    /// Remove and return the head, as it finishes or is skipped.
    pub fn advance(&mut self) -> Option<QueueEntry> {
        self.entries.pop_front()
    }

    pub fn remove(&mut self, passage_id: i64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.passage_id != passage_id);
        self.entries.len() != before
    }

    /// How many more passages to request. Zero when the queue is deep enough.
    ///
    /// The queue must never empty while eligible passages exist `[REQ-PD-100]`,
    /// so this is checked continuously rather than only on advance.
    pub fn shortfall(&self) -> usize {
        self.min_depth.saturating_sub(self.entries.len())
    }

    /// Should the second entry be admitted, given progress through the first?
    pub fn should_admit_next(&self, played_ms: u64) -> bool {
        match (self.peek(), self.peek_next()) {
            (Some(cur), Some(next)) => should_admit(cur, played_ms, next),
            _ => false,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &QueueEntry> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i64, dur_ms: u64, lead_in: u64, lead_out: u64) -> QueueEntry {
        QueueEntry {
            passage_id: id,
            path: PathBuf::from("x.mp3"),
            start_ms: 0,
            end_ms: dur_ms,
            lead_in_ms: lead_in,
            lead_out_ms: lead_out,
            gain_db: 0.0,
            mbid: None,
        }
    }

    /// SPEC002's worked example: A lead-out 3s, B lead-in 5s, overlap 3s.
    #[test]
    fn case_1_following_passage_has_longer_lead_in() {
        let a = entry(1, 60_000, 0, 3_000);
        let b = entry(2, 60_000, 5_000, 0);
        assert_eq!(overlap_ms(&a, &b), 3_000);
    }

    /// SPEC002's worked example: A lead-out 5s, B lead-in 2s, overlap 2s.
    #[test]
    fn case_2_following_passage_has_shorter_lead_in() {
        let a = entry(1, 60_000, 0, 5_000);
        let b = entry(2, 60_000, 2_000, 0);
        assert_eq!(overlap_ms(&a, &b), 2_000);
    }

    #[test]
    fn case_3_zero_lead_durations_do_not_overlap() {
        assert_eq!(overlap_ms(&entry(1, 60_000, 0, 0), &entry(2, 60_000, 0, 0)), 0);
        // one side zero is enough to suppress the overlap
        assert_eq!(overlap_ms(&entry(1, 60_000, 0, 4_000), &entry(2, 60_000, 0, 0)), 0);
    }

    #[test]
    fn overlap_cannot_exceed_either_passage() {
        // a 2s passage cannot sustain a 10s crossfade
        let a = entry(1, 2_000, 0, 10_000);
        let b = entry(2, 60_000, 10_000, 0);
        assert_eq!(overlap_ms(&a, &b), 2_000);
    }

    #[test]
    fn admission_fires_exactly_at_the_lead_out_point() {
        let a = entry(1, 60_000, 0, 4_000);
        let b = entry(2, 60_000, 6_000, 0); // overlap = 4s
        assert!(!should_admit(&a, 55_000, &b), "1s too early");
        assert!(should_admit(&a, 56_000, &b), "exactly at the lead-out point");
        assert!(should_admit(&a, 58_000, &b), "and after it");
    }

    #[test]
    fn no_overlap_admits_only_at_the_end() {
        let a = entry(1, 10_000, 0, 0);
        let b = entry(2, 10_000, 0, 0);
        assert!(!should_admit(&a, 9_999, &b));
        assert!(should_admit(&a, 10_000, &b), "gapless handover at the boundary");
    }

    #[test]
    fn shortfall_drives_replenishment() {
        let mut q = Queue::new(3);
        assert_eq!(q.shortfall(), 3, "empty queue needs a full refill");
        q.push(entry(1, 1000, 0, 0));
        q.push(entry(2, 1000, 0, 0));
        assert_eq!(q.shortfall(), 1);
        q.push(entry(3, 1000, 0, 0));
        assert_eq!(q.shortfall(), 0, "deep enough must ask for nothing");
    }

    #[test]
    fn push_after_current_does_not_interrupt_playback() {
        let mut q = Queue::new(3);
        q.push(entry(1, 1000, 0, 0));
        q.push(entry(2, 1000, 0, 0));
        q.push_after_current(entry(99, 1000, 0, 0));
        let ids: Vec<i64> = q.iter().map(|e| e.passage_id).collect();
        assert_eq!(ids, vec![1, 99, 2], "playing passage must stay at the head");
    }

    #[test]
    fn removing_a_queued_passage_leaves_the_rest_ordered() {
        let mut q = Queue::new(3);
        for i in 1..=4 {
            q.push(entry(i, 1000, 0, 0));
        }
        assert!(q.remove(3));
        assert!(!q.remove(3), "removing twice must report nothing removed");
        let ids: Vec<i64> = q.iter().map(|e| e.passage_id).collect();
        assert_eq!(ids, vec![1, 2, 4]);
    }

    #[test]
    fn a_lone_passage_never_admits() {
        let mut q = Queue::new(3);
        q.push(entry(1, 10_000, 0, 5_000));
        assert!(!q.should_admit_next(10_000), "nothing to admit");
    }
}
