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
    /// This entry's own identity, distinct from the passage it plays.
    ///
    /// A passage may sit in the queue more than once -- deliberately, as a
    /// repeat -- and the two copies are different queue entries that happen to
    /// name the same audio. Addressing the queue by `passage_id` made them
    /// indistinguishable, so removing one removed both and moving one moved
    /// whichever came first. Stamped by [`Queue`] on the way in; `0` until
    /// then, which is the value carried by an entry that has never been queued.
    pub qid: u64,
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
    /// What to call this passage, and how often it has been heard
    /// `[REQ-VIS-170]`. Empty until `Library::describe` fills it; a passage
    /// plays perfectly well unnamed, so nothing here is required.
    pub naming: Naming,
}

/// Names from both sources, kept apart rather than merged on arrival.
///
/// MusicBrainz is preferred where it has an answer, but which source spoke is
/// itself worth showing `[REQ-VIS-120]` -- "Recording title" and "whatever the
/// file's ID3 says" are not the same claim, and collapsing them at load time
/// would throw away the difference before anyone could see it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Naming {
    /// MusicBrainz Recording title -- the name of this performance.
    pub mb_title: Option<String>,
    /// MusicBrainz Artist name, by artist credit.
    pub mb_artist: Option<String>,
    /// MusicBrainz **Release** title, which is what an album name is. Distinct
    /// from the Recording: one recording appears on many releases, so this is
    /// a join away rather than a column, and it stays `None` until those
    /// tables are populated.
    pub mb_album: Option<String>,
    /// The file's own tags, used only where MusicBrainz is silent.
    pub tag_title: Option<String>,
    pub tag_artist: Option<String>,
    pub tag_album: Option<String>,
    /// How many times this recording has been played, over all of history.
    /// Counted by recording, not by passage: the same recording reached
    /// through two files is the same thing heard twice.
    pub plays: i64,
    /// When it was last heard, as a Unix timestamp.
    pub last_played: Option<i64>,
}

/// Where a displayed name came from, so a UI can say so `[REQ-VIS-120]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    MusicBrainz,
    FileTags,
    Filename,
    Unknown,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::MusicBrainz => "musicbrainz",
            Source::FileTags => "tags",
            Source::Filename => "filename",
            Source::Unknown => "unknown",
        }
    }
}

impl Naming {
    pub fn apply_tags(&mut self, t: crate::tags::Tags) {
        self.tag_title = t.title;
        self.tag_artist = t.artist;
        self.tag_album = t.album;
    }
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
    /// What to call this passage: the MusicBrainz Recording title if there is
    /// one, then the file's own tag, and only then the filename
    /// `[REQ-VIS-170]`.
    ///
    /// The filename is the floor rather than the answer -- it produces
    /// "(Heart)Little_Queen-02-Love_Alive", which is a path, not a title.
    pub fn title(&self) -> String {
        self.naming
            .mb_title
            .clone()
            .or_else(|| self.naming.tag_title.clone())
            .unwrap_or_else(|| {
                self.path.file_stem().unwrap_or_default().to_string_lossy().to_string()
            })
    }

    pub fn title_source(&self) -> Source {
        if self.naming.mb_title.is_some() {
            Source::MusicBrainz
        } else if self.naming.tag_title.is_some() {
            Source::FileTags
        } else {
            Source::Filename
        }
    }

    /// The performer. No filename fallback: guessing an artist out of a path is
    /// how a library ends up believing in a band called "02".
    pub fn artist(&self) -> Option<String> {
        self.naming.mb_artist.clone().or_else(|| self.naming.tag_artist.clone())
    }

    pub fn artist_source(&self) -> Source {
        match (&self.naming.mb_artist, &self.naming.tag_artist) {
            (Some(_), _) => Source::MusicBrainz,
            (None, Some(_)) => Source::FileTags,
            _ => Source::Unknown,
        }
    }

    /// The release this passage is from.
    pub fn album(&self) -> Option<String> {
        self.naming.mb_album.clone().or_else(|| self.naming.tag_album.clone())
    }

    pub fn album_source(&self) -> Source {
        match (&self.naming.mb_album, &self.naming.tag_album) {
            (Some(_), _) => Source::MusicBrainz,
            (None, Some(_)) => Source::FileTags,
            _ => Source::Unknown,
        }
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
    /// Stamped onto each entry as it arrives. Monotonic and never reused, so
    /// an identifier a browser is holding can only ever be stale -- naming an
    /// entry that has since played or been removed -- and never ambiguous.
    next_qid: u64,
}

impl Queue {
    pub fn new(min_depth: usize) -> Self {
        Self { entries: VecDeque::new(), min_depth, next_qid: 1 }
    }

    /// Give an entry its identity. Every path into the queue goes through here.
    fn stamp(&mut self, mut e: QueueEntry) -> QueueEntry {
        e.qid = self.next_qid;
        self.next_qid += 1;
        e
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Drop everything waiting.
    ///
    /// For a handoff `[SPEC-BK-030]`: the passages have been read out by id and
    /// are being rebuilt on the other backend, so leaving copies here would
    /// mean two answers to what is coming.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn peek(&self) -> Option<&QueueEntry> {
        self.entries.front()
    }
    pub fn peek_next(&self) -> Option<&QueueEntry> {
        self.entries.get(1)
    }

    /// Append, as the Program Director's selections arrive.
    pub fn push(&mut self, e: QueueEntry) {
        let e = self.stamp(e);
        self.entries.push_back(e);
    }

    /// Insert immediately after the playing passage — a user "play next".
    /// Put a passage at the top of the queue.
    ///
    /// The queue holds only what is still to come: the sounding passage lives
    /// in `live` and is not in here. So the front IS the next thing heard, and
    /// it is what Skip reaches for.
    pub fn push_front(&mut self, e: QueueEntry) {
        let e = self.stamp(e);
        self.entries.push_front(e);
    }

    /// Insert several passages at `at`, keeping the order they arrive in
    /// `[REQ-VIS-195]`.
    ///
    /// One call rather than repeated single inserts, because inserting three
    /// passages one at a time at the same index yields them backwards -- the
    /// kind of bug that looks like a UI fault and is not.
    pub fn insert_at(&mut self, at: usize, entries: Vec<QueueEntry>) {
        let mut at = at.min(self.entries.len());
        for e in entries {
            let e = self.stamp(e);
            self.entries.insert(at, e);
            at += 1;
        }
    }

    /// Move a queued passage `delta` places, clamped to the ends. Returns
    /// whether anything moved -- false when it is already first or last, which
    /// a UI should treat as "nothing to do" rather than as a failure.
    pub fn shift(&mut self, qid: u64, delta: isize) -> bool {
        let Some(at) = self.entries.iter().position(|e| e.qid == qid) else {
            return false;
        };
        let to = (at as isize + delta).clamp(0, self.entries.len() as isize - 1) as usize;
        if to == at {
            return false;
        }
        let e = self.entries.remove(at).expect("index came from position()");
        self.entries.insert(to, e);
        true
    }


    /// Remove and return the head, as it finishes or is skipped.
    pub fn advance(&mut self) -> Option<QueueEntry> {
        self.entries.pop_front()
    }

    pub fn remove(&mut self, qid: u64) -> bool {
        let before = self.entries.len();
        // By entry, not by passage: a repeat is two entries naming one passage,
        // and dropping both is not what anyone asked for.
        self.entries.retain(|e| e.qid != qid);
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

    /// Moving must be clamped, not wrapped: a listener nudging the first
    /// passage "sooner" expects nothing to happen, not for it to jump to last.
    #[test]
    fn shifting_is_clamped_at_both_ends() {
        let mut q = Queue::new(3);
        for id in 1..=3 {
            q.push(entry(id, 1000, 0, 0));
        }
        assert!(!q.shift(1, -1), "already first");
        assert!(!q.shift(3, 1), "already last");
        assert_eq!(ids(&q), vec![1, 2, 3], "and nothing moved");
    }

    #[test]
    fn shifting_moves_one_place_each_way() {
        let mut q = Queue::new(3);
        for id in 1..=4 {
            q.push(entry(id, 1000, 0, 0));
        }
        assert!(q.shift(4, -1));
        assert_eq!(ids(&q), vec![1, 2, 4, 3]);
        assert!(q.shift(1, 1));
        assert_eq!(ids(&q), vec![2, 1, 4, 3]);
        assert!(!q.shift(99, -1), "a passage not queued cannot move");
    }

    /// The front of the queue is the next thing heard, because the sounding
    /// passage is not in the queue at all. Both "now" and "next" therefore go
    /// to index 0; only "now" also skips.
    #[test]
    fn the_front_of_the_queue_is_what_plays_next() {
        let mut q = Queue::new(3);
        q.push(entry(1, 1000, 0, 0));
        q.push(entry(2, 1000, 0, 0));
        q.push_front(entry(99, 1000, 0, 0));
        assert_eq!(ids(&q), vec![99, 1, 2]);
    }

    /// The trap this method exists for: inserting three passages one at a time
    /// at the same index yields them backwards. A whole album queued in reverse
    /// looks like a UI fault and is not `[REQ-VIS-195]`.
    #[test]
    fn a_batch_keeps_the_order_it_was_given() {
        let mut q = Queue::new(3);
        q.push(entry(1, 1000, 0, 0));
        q.push(entry(2, 1000, 0, 0));
        q.insert_at(1, (10..13).map(|id| entry(id, 1000, 0, 0)).collect());
        assert_eq!(ids(&q), vec![1, 10, 11, 12, 2]);
    }

    #[test]
    fn a_batch_past_the_end_appends_rather_than_panicking() {
        let mut q = Queue::new(3);
        q.push(entry(1, 1000, 0, 0));
        q.insert_at(99, (10..12).map(|id| entry(id, 1000, 0, 0)).collect());
        assert_eq!(ids(&q), vec![1, 10, 11]);
    }

    fn ids(q: &Queue) -> Vec<i64> {
        q.iter().map(|e| e.passage_id).collect()
    }

    fn named(mb: Option<&str>, tag: Option<&str>) -> QueueEntry {
        let mut e = entry(1, 1000, 0, 0);
        e.path = PathBuf::from("/music/(Heart)Little_Queen-02-Love_Alive.mp3");
        e.naming.mb_title = mb.map(String::from);
        e.naming.tag_title = tag.map(String::from);
        e
    }

    /// MusicBrainz first. That is the whole point of identifying a passage:
    /// the Recording title is the name of the performance, where a tag is
    /// whatever whoever ripped the disc happened to type.
    #[test]
    fn the_recording_title_is_preferred_over_the_file_tag() {
        assert_eq!(named(Some("Love Alive"), Some("love alive")).title(), "Love Alive");
        assert_eq!(named(Some("Love Alive"), None).title_source(), Source::MusicBrainz);
    }

    #[test]
    fn the_file_tag_is_used_when_musicbrainz_is_silent() {
        let e = named(None, Some("Love Alive"));
        assert_eq!(e.title(), "Love Alive");
        assert_eq!(e.title_source(), Source::FileTags);
    }

    /// The filename is the floor, not the answer -- it yields a path, not a
    /// title -- but an unidentified, untagged passage must still show
    /// something and still play.
    #[test]
    fn the_filename_is_the_last_resort() {
        let e = named(None, None);
        assert_eq!(e.title(), "(Heart)Little_Queen-02-Love_Alive");
        assert_eq!(e.title_source(), Source::Filename);
    }

    /// Artist and album have no filename fallback: guessing a performer out of
    /// a path is how a library comes to believe in a band called "02".
    #[test]
    fn artist_and_album_are_absent_rather_than_guessed() {
        let e = named(None, None);
        assert_eq!(e.artist(), None);
        assert_eq!(e.album(), None);
        assert_eq!(e.artist_source(), Source::Unknown);
        assert_eq!(e.album_source(), Source::Unknown);
    }

    /// Album is the Release title, and the release tables are empty until
    /// Sampo fills them -- so today this path is always the tag one, and it
    /// must say so rather than claim MusicBrainz.
    #[test]
    fn each_field_reports_its_own_source() {
        let mut e = entry(1, 1000, 0, 0);
        e.naming.mb_title = Some("Recording Title".into());
        e.naming.mb_artist = Some("The Artist".into());
        e.naming.tag_album = Some("Some Compilation".into());
        assert_eq!(e.title_source(), Source::MusicBrainz);
        assert_eq!(e.artist_source(), Source::MusicBrainz);
        assert_eq!(e.album_source(), Source::FileTags);
        assert_eq!(e.album().as_deref(), Some("Some Compilation"));
    }

    /// A passage may be queued twice on purpose. The two are separate entries
    /// that happen to name the same audio, and this is what MuLibPlay got
    /// wrong: removing one removed both `[REQ-VIS-186]`.
    #[test]
    fn removing_one_copy_of_a_repeated_passage_leaves_the_other() {
        let mut q = Queue::new(1);
        q.push(entry(7, 1000, 0, 0));
        q.push(entry(9, 1000, 0, 0));
        q.push(entry(7, 1000, 0, 0)); // the same passage again, deliberately
        let first = q.iter().next().unwrap().qid;
        let second_seven = q.iter().nth(2).unwrap().qid;
        assert_ne!(first, second_seven, "two entries must not share an identity");

        assert!(q.remove(first));
        let left: Vec<i64> = q.iter().map(|e| e.passage_id).collect();
        assert_eq!(left, vec![9, 7], "only the named entry may go");
        assert!(q.iter().any(|e| e.qid == second_seven), "the other copy survives");
    }

    /// And moving one must move THAT one, not whichever copy comes first.
    #[test]
    fn shifting_one_copy_moves_that_copy() {
        let mut q = Queue::new(1);
        q.push(entry(7, 1000, 0, 0));
        q.push(entry(9, 1000, 0, 0));
        q.push(entry(7, 1000, 0, 0));
        let last_seven = q.iter().nth(2).unwrap().qid;

        assert!(q.shift(last_seven, -1));
        let order: Vec<u64> = q.iter().map(|e| e.qid).collect();
        assert_eq!(order[1], last_seven, "the named entry moved up one");
        assert_eq!(q.iter().map(|e| e.passage_id).collect::<Vec<_>>(), vec![7, 7, 9]);
    }

    /// An identifier a browser is holding may be stale -- the entry played, or
    /// was removed -- and that must be a quiet no-op, not a wrong edit.
    #[test]
    fn a_stale_entry_id_touches_nothing() {
        let mut q = Queue::new(1);
        q.push(entry(7, 1000, 0, 0));
        let gone = q.iter().next().unwrap().qid;
        assert!(q.remove(gone));
        assert!(!q.remove(gone), "removing it twice must report nothing done");
        assert!(!q.shift(gone, 1));
        q.push(entry(8, 1000, 0, 0));
        assert!(!q.remove(gone), "a reused number would hit the new entry");
        assert_eq!(q.len(), 1);
    }

    fn entry(id: i64, dur_ms: u64, lead_in: u64, lead_out: u64) -> QueueEntry {
        QueueEntry {
            qid: 0, // stamped by Queue on the way in
            passage_id: id,
            path: PathBuf::from("x.mp3"),
            start_ms: 0,
            end_ms: dur_ms,
            lead_in_ms: lead_in,
            lead_out_ms: lead_out,
            gain_db: 0.0,
            mbid: None,
            naming: Default::default(),
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

    /// This test used to assert the opposite, with the comment "playing passage
    /// must stay at the head" -- and that belief is where the bug came from.
    /// The head of the queue is not the playing passage; the playing passage is
    /// in `live` and not in the queue at all, so inserting after the head put
    /// everything one place too late.
    #[test]
    fn the_queue_holds_only_what_is_still_to_come() {
        let mut q = Queue::new(3);
        q.push(entry(1, 1000, 0, 0));
        q.push(entry(2, 1000, 0, 0));
        q.push_front(entry(99, 1000, 0, 0));
        let ids: Vec<i64> = q.iter().map(|e| e.passage_id).collect();
        assert_eq!(ids, vec![99, 1, 2], "the front is the next thing heard");
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
