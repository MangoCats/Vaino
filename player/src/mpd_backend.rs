//! MPD as a backend the session can drive `[SPEC-BK-020]`.
//!
//! `mpd_direct` proved this as a program with its own loop. This is the same
//! behaviour turned inside out: the loop belongs to the session now, and what
//! remains is an implementation of [`Playback`] that happens to reach a server
//! instead of a sound card.
//!
//! **The rate limit lives here, not in the caller.** `tick` is called as often
//! as the engine thread spins, and MPD must be sampled every few seconds
//! `[SPEC-MPD-105]` rather than every few microseconds. A backend that made the
//! session responsible for its polling rate would have leaked its own protocol
//! upward, which is the thing the trait exists to prevent.
//!
//! **Not playing means nothing to add.** `shortfall` reports zero unless MPD is
//! playing, so `[SPEC-MPD-120]`'s activation rule needs no special case in the
//! session: the Director simply finds nothing wanted.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::db::{PlayerStore, Rejection};
use crate::mpd::{quote, Mpd};
use crate::playback::{Capabilities, Playback};
use crate::queue::QueueEntry;
use crate::scrobble::counts_as_play;

/// The URI MPD would use for a path, or `None` if it is outside MPD's tree.
///
/// A free function because naming needs a root and a path and **not a socket**:
/// testing it through a connected backend would have meant faking a TCP stream
/// to check a string operation.
fn uri_under(root: &str, path: &std::path::Path) -> Option<String> {
    let norm = path.to_string_lossy().replace('\\', "/");
    norm.strip_prefix(root).map(|r| r.trim_start_matches('/').to_string())
}

/// What a URI names, when it names exactly one thing.
///
/// `None` means **ambiguous**, which is not the same as unknown and must not be
/// treated as it: the file is in the library, it simply carries more than one
/// radio passage, and a whole-file entry could be any of them `[SPEC-MPD-060]`.
pub type Nameable = Option<(i64, Option<String>, u64)>;

/// Build the URI → passage map a backend needs to adopt a person's own
/// additions `[SPEC-MPD-115]`.
///
/// A file carrying more than one radio passage is recorded as ambiguous rather
/// than resolved to one of them. Added whole, a DAO capture is forty songs, and
/// attributing a play to whichever passage happened to be first would credit
/// one nobody heard.
pub fn nameable_uris(
    conn: &rusqlite::Connection,
    root: &str,
) -> Result<HashMap<String, Nameable>, String> {
    let root = root.replace('\\', "/").trim_end_matches('/').to_string();
    let mut q = conn
        .prepare(
            "SELECT f.path, f.duration_ms, p.passage_id,                 (SELECT pr.mbid FROM passage_recordings pr                  WHERE pr.passage_id = p.passage_id LIMIT 1)              FROM passages p JOIN files f USING(file_id) WHERE p.kind = 'radio'",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?.unwrap_or(0) as u64,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out: HashMap<String, Nameable> = HashMap::new();
    for (path, dur, pid, mbid) in rows.flatten() {
        let norm = path.replace('\\', "/");
        let Some(rel) = norm.strip_prefix(&root) else { continue };
        let uri = rel.trim_start_matches('/').to_string();
        match out.entry(uri) {
            std::collections::hash_map::Entry::Occupied(mut e) => {
                e.insert(None); // a second passage: ambiguous
            }
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(Some((pid, mbid, dur)));
            }
        }
    }
    Ok(out)
}

/// A passage handed to MPD, and what has become of it.
struct Offered {
    passage_id: i64,
    /// The URI it was added under. A sticker is addressed by URI, and the
    /// reasoning arrives after the add.
    uri: String,
    mbid: Option<String>,
    /// The span from Vaino, never MPD's estimate of it `[SPEC-MPD-092]`.
    span_ms: u64,
    furthest_ms: u64,
    /// False when MPD dropped the range end `[SPEC-MPD-096]`, so this backend
    /// must end the passage itself rather than let it run to end of file.
    span_honoured: bool,
    /// Whether it ever reached the front. The whole difference between a
    /// listener hearing it and a listener removing it, and MPD reports the
    /// departure identically either way.
    was_current: bool,
}

pub struct MpdBackend {
    mpd: Mpd,
    /// MPD's `music_directory`, normalised, for turning a path into a URI.
    root: String,
    depth: usize,
    interval: Duration,
    last_poll: Option<Instant>,
    /// songid → what we offered under it.
    ours: HashMap<String, Offered>,
    /// MPD's whole queue length at the last poll, ours and the listener's both.
    queue_len: usize,
    playing: bool,
    dropped: Vec<i64>,
    store: Option<PlayerStore>,
    /// URI → passage, for adopting what the listener queued `[SPEC-MPD-115]`.
    names: HashMap<String, Nameable>,
    /// Reported once each, so a log is not a stream of the same complaint.
    unnameable: HashSet<String>,
    lost: bool,
}

impl MpdBackend {
    pub fn connect(
        addr: &str,
        music_root: &str,
        depth: usize,
        interval_ms: u64,
    ) -> Result<Self, String> {
        let mut mpd = Mpd::connect(addr)?;
        // `consume 1` is what makes MPD's queue the same object as Vaino's
        // `[SPEC-MPD-035]`: a played passage leaves, so "top up to depth" is a
        // stable statement rather than a growing playlist.
        mpd.cmd("consume 1")?;
        Ok(Self {
            mpd,
            root: music_root.replace('\\', "/").trim_end_matches('/').to_string(),
            depth,
            interval: Duration::from_millis(interval_ms),
            last_poll: None,
            ours: HashMap::new(),
            queue_len: 0,
            playing: false,
            dropped: Vec::new(),
            store: None,
            names: HashMap::new(),
            unnameable: HashSet::new(),
            lost: false,
        })
    }

    /// Teach it to name what the listener queues `[SPEC-MPD-115]`. Without
    /// this, a song added by hand plays and counts for nothing.
    pub fn attach_names(&mut self, names: HashMap<String, Nameable>) {
        self.names = names;
    }

    /// Where plays and rejections are written `[SPEC-PLAY-030]`. Optional, so a
    /// dry run is a run without a store rather than a flag threaded through.
    pub fn attach_store(&mut self, store: PlayerStore) {
        self.store = Some(store);
    }

    fn uri_for(&self, e: &QueueEntry) -> Option<String> {
        uri_under(&self.root, &e.path)
    }

    /// Every URI in MPD's queue, in play order — **including what a person
    /// added by hand**.
    ///
    /// Not on the trait, because only a handoff wants it. `queued_ids` reports
    /// what the Director offered; this reports what the *listener* would see,
    /// and the difference is exactly the songs Vaino did not choose. Carrying
    /// only our own would silently discard theirs `[SPEC-BK-045]`.
    pub fn queue_uris(&mut self) -> Vec<String> {
        match self.mpd.cmd("playlistinfo") {
            Ok(lines) => lines
                .iter()
                .filter_map(|l| l.strip_prefix("file: "))
                .map(|s| s.to_string())
                .collect(),
            Err(e) => {
                eprintln!("mpd playlistinfo: {e}");
                Vec::new()
            }
        }
    }

    /// Fade out and stop, returning whether the fade was real.
    ///
    /// **MPD cannot be asked to fade.** The protocol has a global `crossfade`
    /// between songs and it has `stop`; there is no "fade out over N ms". So a
    /// fade has to be built from `setvol` steps — and `setvol` is refused with
    /// **`No mixer`** unless the output plugin has one `[SPEC-MPD-099]`.
    ///
    /// Measured: MPD's `null` output has no mixer at all, so on a test rig this
    /// returns `false` and cuts. A PipeWire or ALSA output — what an appliance
    /// actually runs — does have one. Reporting which happened is the point:
    /// silently cutting where a fade was promised is the class of lie
    /// `[PI3-API-030]` refuses.
    ///
    /// The listener's volume is **restored** afterwards. It is theirs, their
    /// other clients display it, and borrowing it for a second is only
    /// acceptable if it is given back.
    fn fade_out_inner(&mut self, ms: u64) -> bool {
        let start = self
            .mpd
            .status()
            .ok()
            .and_then(|s| s.get("volume").and_then(|v| v.parse::<i64>().ok()))
            .filter(|v| *v >= 0);
        let Some(start) = start else {
            self.stop_sounding();
            return false;
        };
        const STEPS: u64 = 8;
        let step = Duration::from_millis((ms / STEPS).max(1));
        for i in 1..=STEPS {
            let level = start - (start * i as i64 / STEPS as i64);
            if self.mpd.cmd(&format!("setvol {}", level.max(0))).is_err() {
                // The mixer went away mid-fade. Stop rather than leave the
                // listener's volume somewhere they did not put it.
                break;
            }
            std::thread::sleep(step);
        }
        self.stop_sounding();
        let _ = self.mpd.cmd(&format!("setvol {start}"));
        true
    }

    /// Stop sounding, without losing the connection.
    ///
    /// A handoff away from MPD leaves MPD *running* — it is a guest that may be
    /// wanted again in a moment, and its clients are still someone's remote
    /// control `[SPEC-BK-025]`.
    pub fn stop_sounding(&mut self) {
        let _ = self.mpd.cmd("stop");
        self.playing = false;
    }

    /// Judge a passage that has left MPD's queue, and record what it was.
    fn retire(&mut self, o: Offered) {
        if !o.was_current {
            // Never reached the front, so a person took it out. It did not play
            // and it is not forgotten: the removal earns the shorter window
            // `[SPEC-PLAY-055]`, and the Director is told to un-count the
            // queueing mark by way of `take_dropped` `[REQ-PD-112]`.
            self.dropped.push(o.passage_id);
            self.write(Rejection::Dequeue, o.passage_id, o.mbid.as_deref());
            return;
        }
        // It sounded. Whether it *played* is `[SPEC-PLAY-010]`'s question, and
        // it is asked against Vaino's span rather than MPD's idea of one.
        if counts_as_play(o.furthest_ms, o.span_ms) {
            if let Some(s) = &self.store {
                if let Err(e) = s.record_play(o.passage_id, o.mbid.as_deref()) {
                    eprintln!("record play: {e}");
                }
            }
        } else {
            self.write(Rejection::Skip, o.passage_id, o.mbid.as_deref());
        }
    }

    fn write(&self, kind: Rejection, passage_id: i64, mbid: Option<&str>) {
        if let Some(s) = &self.store {
            if let Err(e) = s.record_rejection(kind, passage_id, mbid) {
                eprintln!("record {}: {e}", kind.as_str());
            }
        }
    }

    /// One sample: where playback is, what has left the queue, and whether a
    /// span MPD would not honour has run past its end.
    fn poll(&mut self) {
        let status = match self.mpd.status() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("mpd status: {e}");
                self.lost = true;
                return;
            }
        };
        self.playing = status.get("state").map(|s| s == "play").unwrap_or(false);
        self.queue_len =
            status.get("playlistlength").and_then(|v| v.parse().ok()).unwrap_or(0);
        let current = status.get("songid").cloned();
        let elapsed_ms = status
            .get("elapsed")
            .and_then(|v| v.parse::<f64>().ok())
            .map(|s| (s * 1000.0).round() as u64)
            .unwrap_or(0);

        // **A song this backend did not queue is still one the listener heard**
        // `[SPEC-MPD-115]`. Adopt it once, so its play is attributed and its
        // artist blocks like any other. Without this a person's own additions
        // sound and count for nothing.
        if let Some(id) = current.clone() {
            if !self.ours.contains_key(&id) {
                let uri = self
                    .mpd
                    .cmd("currentsong")
                    .map(|l| crate::mpd::parse(&l).get("file").cloned().unwrap_or_default())
                    .unwrap_or_default();
                match self.names.get(&uri) {
                    Some(Some((pid, mbid, file_ms))) => {
                        self.ours.insert(
                            id.clone(),
                            Offered {
                                passage_id: *pid,
                                uri: uri.clone(),
                                mbid: mbid.clone(),
                                // Queued whole, so the file's length is the
                                // span; there is no range to be relative to.
                                span_ms: *file_ms,
                                furthest_ms: 0,
                                span_honoured: true,
                                was_current: true,
                            },
                        );
                    }
                    // Ambiguous, or not in the library at all. Reported once
                    // rather than guessed at `[SPEC-MPD-060]`, and once rather
                    // than on every sample.
                    _ if !uri.is_empty() && self.unnameable.insert(uri.clone()) => {
                        eprintln!(
                            "a queued song could not be named, so its play is not attributed: {}",
                            uri.rsplit('/').next().unwrap_or(&uri)
                        );
                    }
                    _ => {}
                }
            }
        }

        let mut overrun = false;
        if let Some(id) = &current {
            if let Some(o) = self.ours.get_mut(id) {
                o.was_current = true;
                if self.playing {
                    o.furthest_ms = o.furthest_ms.max(elapsed_ms);
                }
                // MPD will play to end of file where it dropped the range end,
                // so the passage is ended here instead `[SPEC-MPD-096]`. The
                // overrun is then bounded by the sample interval rather than by
                // the length of the file.
                overrun = !o.span_honoured && self.playing && o.furthest_ms >= o.span_ms;
            }
        }
        if overrun {
            let _ = self.mpd.cmd("next");
        }

        let live: HashSet<String> = match self.mpd.cmd("playlistid") {
            Ok(lines) => lines
                .iter()
                .filter_map(|l| l.strip_prefix("Id: "))
                .map(|s| s.to_string())
                .collect(),
            Err(e) => {
                eprintln!("mpd playlistid: {e}");
                self.lost = true;
                return;
            }
        };
        let gone: Vec<String> =
            self.ours.keys().filter(|id| !live.contains(*id)).cloned().collect();
        for id in gone {
            if let Some(o) = self.ours.remove(&id) {
                self.retire(o);
            }
        }
    }
}

/// **The whole point of the sticker approach** `[SPEC-MPD-050]`: a client that
/// has never heard of Vaino is unaffected, and one that shows stickers gains a
/// "why this track" panel without a line of code changing. Nothing is added to
/// MPD to make it work.
impl crate::switch::Publish for MpdBackend {
    fn publish_reasoning(&mut self, passage_id: i64, why: &str, flavor: &str, at: i64) {
        let Some(uri) = self
            .ours
            .values()
            .find(|o| o.passage_id == passage_id)
            .map(|o| o.uri.clone())
        else {
            return; // never offered here, so there is nothing to hang it on
        };
        let mut set = |name: &str, value: &str| {
            if value.is_empty() {
                return;
            }
            let cmd = format!(
                "sticker set song {} {} {}",
                quote(&uri),
                quote(name),
                quote(value)
            );
            if let Err(e) = self.mpd.cmd(&cmd) {
                eprintln!("sticker {name}: {e}");
            }
        };
        set("vaino.why", why);
        set("vaino.flavor", flavor);
        set("vaino.chosen_at", &at.to_string());
    }
}

impl crate::switch::FadeOut for MpdBackend {
    fn fade_out(&mut self, ms: u64) -> crate::switch::Stopped {
        if self.fade_out_inner(ms) {
            crate::switch::Stopped::Faded
        } else {
            crate::switch::Stopped::Cut
        }
    }
}

impl Playback for MpdBackend {
    /// Spans yes, gain and ramps no `[SPEC-MPD-055]`. Declared rather than
    /// discovered when a passage plays at the wrong level.
    fn capabilities(&self) -> Capabilities {
        Capabilities::MPD
    }

    fn enqueue(&mut self, entry: QueueEntry) {
        let Some(uri) = self.uri_for(&entry) else {
            // Outside MPD's music directory, so MPD cannot name it. It never
            // played, and the Director must un-count it `[REQ-PD-112]`.
            eprintln!("passage {} is not under MPD's music directory", entry.passage_id);
            self.dropped.push(entry.passage_id);
            return;
        };
        match self.mpd.add_ranged(&uri, entry.start_ms as i64, entry.end_ms as i64) {
            Ok(added) => {
                let _ = self.mpd.cmd(&format!(
                    "sticker set song {} {} {}",
                    quote(&uri),
                    quote("vaino.passage"),
                    quote(&entry.passage_id.to_string())
                ));
                self.ours.insert(
                    added.id,
                    Offered {
                        passage_id: entry.passage_id,
                        uri: uri.clone(),
                        mbid: entry.mbid.clone(),
                        span_ms: entry.end_ms.saturating_sub(entry.start_ms),
                        furthest_ms: 0,
                        span_honoured: added.span_honoured,
                        was_current: false,
                    },
                );
                self.queue_len += 1;
            }
            Err(e) => {
                eprintln!("mpd enqueue {}: {e}", entry.passage_id);
                self.dropped.push(entry.passage_id);
            }
        }
    }

    fn queued_ids(&self) -> Vec<i64> {
        self.ours.values().map(|o| o.passage_id).collect()
    }

    fn queued_ms(&self) -> u64 {
        self.ours.values().map(|o| o.span_ms.saturating_sub(o.furthest_ms)).sum()
    }

    /// Zero unless MPD is playing `[SPEC-MPD-120]`, so the activation rule
    /// needs no special case anywhere else. Measured against MPD's **whole**
    /// queue, not only what this backend put there: a person who queued twenty
    /// tracks by hand has left nothing wanted `[SPEC-MPD-095]`.
    fn shortfall(&self) -> usize {
        if !self.playing {
            return 0;
        }
        self.depth.saturating_sub(self.queue_len)
    }

    fn take_dropped(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.dropped)
    }

    fn resume_at(&mut self, position_ms: u64) {
        let first = self.ours.keys().next().cloned();
        if let Some(id) = first {
            let _ = self.mpd.cmd(&format!("seekid {id} {:.3}", position_ms as f64 / 1000.0));
        }
    }

    fn tick(&mut self) -> usize {
        // The session spins far faster than MPD should be asked anything.
        let due = self.last_poll.map(|t| t.elapsed() >= self.interval).unwrap_or(true);
        if due && !self.lost {
            self.last_poll = Some(Instant::now());
            self.poll();
        }
        0
    }

    fn is_shutdown(&self) -> bool {
        self.lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A URI is the path with the music directory taken off the front — rung 1
    /// of the ladder `[SPEC-MPD-060]`, and the only rung this backend uses when
    /// it is the one doing the naming.
    #[test]
    fn a_path_under_the_root_becomes_a_uri() {
        let root = "C:/Users/x/Music";
        let p = std::path::Path::new;
        assert_eq!(uri_under(root, p("C:/Users/x/Music/A/b.mp3")).as_deref(), Some("A/b.mp3"));
        // Backslashes are what the library actually stores on this platform.
        let windows_style = "C:\\Users\\x\\Music\\A\\b.mp3";
        assert_eq!(uri_under(root, p(windows_style)).as_deref(), Some("A/b.mp3"));
        assert_eq!(
            uri_under(root, p("D:/Elsewhere/c.mp3")),
            None,
            "outside the root is not nameable, and must not be guessed at"
        );
    }
}
