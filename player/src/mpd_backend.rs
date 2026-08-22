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

/// A passage handed to MPD, and what has become of it.
struct Offered {
    passage_id: i64,
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
            lost: false,
        })
    }

    /// Where plays and rejections are written `[SPEC-PLAY-030]`. Optional, so a
    /// dry run is a run without a store rather than a flag threaded through.
    pub fn attach_store(&mut self, store: PlayerStore) {
        self.store = Some(store);
    }

    fn uri_for(&self, e: &QueueEntry) -> Option<String> {
        uri_under(&self.root, &e.path)
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
