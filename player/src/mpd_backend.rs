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

/// How far short of the span's end a seek is allowed to land `[SPEC-BK-055]`.
///
/// MPD stops a song seeked past its span outright, and "past" includes the
/// last instant of it. A second short is inaudible and cannot trip that.
const SEEK_MARGIN_MS: u64 = 1_000;

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

/// passage id → the cue track that names it, e.g.
/// `Rush/TestForEcho.cue/track0002` `[SPEC-MPD-056]`.
///
/// Only for captures whose sheet is actually on disk. The track number is the
/// passage's position among that file's radio passages ordered by start, which
/// is exactly how [`crate::cue`] numbered them — the two must agree, and this
/// is the place they do.
pub fn cue_uris(
    conn: &rusqlite::Connection,
    root: &str,
) -> Result<HashMap<i64, String>, String> {
    let root_norm = root.replace('\\', "/").trim_end_matches('/').to_string();
    let mut q = conn
        .prepare(
            "SELECT f.path, p.passage_id FROM passages p JOIN files f USING(file_id)              WHERE p.kind = 'radio' ORDER BY f.path, p.start_ms",
        )
        .map_err(|e| e.to_string())?;
    let rows = q
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?;
    let mut per_file: HashMap<String, Vec<i64>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (path, pid) in rows.flatten() {
        if !per_file.contains_key(&path) {
            order.push(path.clone());
        }
        per_file.entry(path).or_default().push(pid);
    }
    let mut out = HashMap::new();
    for path in order {
        let ids = &per_file[&path];
        // One passage needs no sheet, and `cue::generate` wrote none.
        if ids.len() < 2 {
            continue;
        }
        let cue_path = std::path::Path::new(&path).with_extension("cue");
        if !cue_path.exists() {
            continue;
        }
        let norm = cue_path.to_string_lossy().replace('\\', "/");
        let Some(rel) = norm.strip_prefix(&root_norm) else { continue };
        let base = rel.trim_start_matches('/');
        for (i, pid) in ids.iter().enumerate() {
            out.insert(*pid, format!("{base}/track{:04}", i + 1));
        }
    }
    Ok(out)
}

/// The passages this backend is holding, **in the listener's order**.
///
/// A free function because it is a question about two data structures and not
/// about a socket: testing it through a connected backend would have meant
/// faking a TCP stream to check an ordering.
///
/// Order matters because this is what crosses a handoff `[SPEC-BK-030]`, and
/// `carry_queue` re-enqueues in the order it is given. Reading it out of a
/// `HashMap` shuffled the queue every time a listener moved back to Vaino.
fn in_queue_order(
    order: &[(String, String)],
    ours: &HashMap<String, Offered>,
    names: &HashMap<String, Nameable>,
) -> Vec<i64> {
    order
        .iter()
        .filter_map(|(id, uri)| match ours.get(id) {
            Some(o) => Some(o.passage_id),
            // Not offered by Vaino, so it is the listener's own. Named where
            // the library can name it, and passed over where it cannot -- which
            // is `[SPEC-BK-045]`'s rule, applied where the names actually live.
            None => match names.get(uri) {
                Some(Some((pid, ..))) => Some(*pid),
                _ => None,
            },
        })
        .collect()
}

/// `playlistinfo` as `(songid, uri)` pairs, in MPD's order.
///
/// One song's fields arrive as a run of lines beginning with `file:`, so a
/// `file:` opens an entry and the `Id:` that follows belongs to it.
fn queue_of(lines: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Some(uri) = line.strip_prefix("file: ") {
            out.push((String::new(), uri.to_string()));
        } else if let Some(id) = line.strip_prefix("Id: ") {
            if let Some(last) = out.last_mut() {
                last.0 = id.to_string();
            }
        }
    }
    out
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
    /// The furthest point reached, which is a **position** and is what says
    /// whether the span has run out `[SPEC-MPD-096]`.
    furthest_ms: u64,
    /// How much of it was actually **heard** `[SPEC-PLAY-012]`, credited from
    /// the gap between samples. A seek moves the position without anyone
    /// listening to the distance, so the two part company the moment the
    /// listener touches the bar.
    heard_ms: u64,
    /// The position at the previous sample, for measuring that gap.
    seen_at_ms: Option<u64>,
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
    /// MPD's queue as of the last poll: `(songid, uri)`, in MPD's order.
    ///
    /// **`ours` is a map and a map has no order**, so the listener's order has
    /// to be kept somewhere. `playlistinfo` returns both for one round trip —
    /// the same trip `playlistid` was making to find departures, and it was
    /// throwing the order away into a set on the way past.
    order: Vec<(String, String)>,
    /// MPD's whole queue length at the last poll, ours and the listener's both.
    queue_len: usize,
    playing: bool,
    dropped: Vec<i64>,
    store: Option<PlayerStore>,
    /// URI → passage, for adopting what the listener queued `[SPEC-MPD-115]`.
    names: HashMap<String, Nameable>,
    /// Cue tracks that name a passage a guest could not `[SPEC-MPD-056]`.
    cues: HashMap<i64, String>,
    /// Reported once each, so a log is not a stream of the same complaint.
    unnameable: HashSet<String>,
    /// The passage sounding at the last poll and how far into its span
    /// `[SPEC-BK-065]`. Remembered rather than asked for on demand: a
    /// handoff wants it at a moment when a round trip is the wrong thing to
    /// be doing.
    head: Option<(i64, u64)>,
    /// Passages that arrived mid-play with their play already in the
    /// history. Neither counted again nor written down as a rejection when
    /// they end `[SPEC-BK-065]`.
    counted_elsewhere: HashSet<i64>,
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
            order: Vec::new(),
            queue_len: 0,
            playing: false,
            dropped: Vec::new(),
            store: None,
            names: HashMap::new(),
            cues: HashMap::new(),
            unnameable: HashSet::new(),
            head: None,
            counted_elsewhere: HashSet::new(),
            lost: false,
        })
    }

    /// Teach it to name what the listener queues `[SPEC-MPD-115]`. Without
    /// this, a song added by hand plays and counts for nothing.
    pub fn attach_names(&mut self, names: HashMap<String, Nameable>) {
        self.names = names;
    }

    /// Teach it which passages have a cue track `[SPEC-MPD-056]`.
    pub fn attach_cues(&mut self, cues: HashMap<i64, String>) {
        self.cues = cues;
    }

    /// Where plays and rejections are written `[SPEC-PLAY-030]`. Optional, so a
    /// dry run is a run without a store rather than a flag threaded through.
    pub fn attach_store(&mut self, store: PlayerStore) {
        self.store = Some(store);
    }

    fn uri_for(&self, e: &QueueEntry) -> Option<String> {
        uri_under(&self.root, &e.path)
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
        // A passage handed over mid-play brought its play with it. Asking the
        // question again could only answer it a second time, and answering
        // `no` would be worse still: a rejection for a passage that played
        // `[SPEC-BK-065]`.
        if self.counted_elsewhere.remove(&o.passage_id) {
            return;
        }
        // It sounded. Whether it *played* is `[SPEC-PLAY-010]`'s question, and
        // it is asked against Vaino's span rather than MPD's idea of one.
        if counts_as_play(o.heard_ms, o.span_ms) {
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
        // **MPD's clock on a bounded song runs from the start of the span**,
        // measured rather than assumed `[SPEC-BK-055]`: a `rangeid` range
        // reports `duration` as the range's and `elapsed` from zero, and a cue
        // track does the same. So this is already a position within the
        // passage and needs no adjusting.
        self.head = match (&current, self.playing) {
            (Some(id), true) => self.ours.get(id).map(|o| (o.passage_id, elapsed_ms)),
            _ => None,
        };

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
                                heard_ms: 0,
                                seen_at_ms: None,
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
                    // Only forward movement, and only as much as one sample
                    // could have covered: a jump longer than the interval is
                    // a seek, and nobody heard the distance `[SPEC-PLAY-012]`.
                    let step = self.interval.as_millis() as u64 * 2;
                    if let Some(previous) = o.seen_at_ms {
                        o.heard_ms += elapsed_ms.saturating_sub(previous).min(step);
                    }
                    o.seen_at_ms = Some(elapsed_ms);
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

        let order = match self.mpd.cmd("playlistinfo") {
            Ok(lines) => queue_of(&lines),
            Err(e) => {
                eprintln!("mpd playlistinfo: {e}");
                self.lost = true;
                return;
            }
        };
        let live: HashSet<&str> = order.iter().map(|(id, _)| id.as_str()).collect();
        let gone: Vec<String> =
            self.ours.keys().filter(|id| !live.contains(id.as_str())).cloned().collect();
        self.order = order;
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
/// Where MPD says it is, from the last poll — and able to ask again now,
/// because a handoff cannot wait for the next scheduled one `[SPEC-BK-065]`.
impl crate::switch::Progress for MpdBackend {
    fn head_position(&self) -> Option<(i64, u64)> {
        self.head
    }

    fn adopt_counted(&mut self, passage_id: i64) {
        self.counted_elsewhere.insert(passage_id);
    }

    fn refresh(&mut self) {
        if !self.lost {
            self.last_poll = Some(Instant::now());
            self.poll();
        }
    }
}

impl crate::switch::Publish for MpdBackend {
    fn publish(&mut self, p: &crate::switch::Published<'_>) {
        let Some(uri) = self
            .ours
            .values()
            .find(|o| o.passage_id == p.passage_id)
            .map(|o| o.uri.clone())
        else {
            return; // never offered here, so there is nothing to hang it on
        };
        let mut set = |name: &str, value: &str| {
            if value.is_empty() {
                return;
            }
            let cmd =
                format!("sticker set song {} {} {}", quote(&uri), quote(name), quote(value));
            if let Err(e) = self.mpd.cmd(&cmd) {
                eprintln!("sticker {name}: {e}");
            }
        };
        set("vaino.why", p.why);
        set("vaino.flavor", p.flavor);
        // **Identity, because MPD cannot carry it** `[SPEC-MPD-052]`. A
        // capture's file tags name the album and no track, so a client reading
        // tags alone shows the album for every passage inside it. This is the
        // only place the real title exists on this side.
        set("vaino.title", p.title);
        set("vaino.artist", p.artist);
        set("vaino.chosen_at", &p.chosen_at.to_string());
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
        let span_ms = entry.end_ms.saturating_sub(entry.start_ms);

        // **A cue track names what a bare file cannot** `[SPEC-MPD-056]`. MPD
        // applies the cue's own boundaries as a range and reports the real
        // title, so `rangeid` is not called here — it would overwrite that
        // range with offsets into the *file* and lose both.
        //
        // The cue's end is the next track's start, which is a median 4.8 s
        // late against a radio span, so the passage is marked as one MPD will
        // not end correctly and the sampler ends it instead `[SPEC-MPD-096]`.
        // That is the same machinery, bounding the overrun by one interval.
        if let Some(uri) = self.cues.get(&entry.passage_id).cloned() {
            match self.mpd.cmd(&format!("addid {}", quote(&uri))) {
                Ok(lines) => {
                    if let Some(id) = crate::mpd::parse(&lines).get("Id").cloned() {
                        self.ours.insert(
                            id,
                            Offered {
                                passage_id: entry.passage_id,
                                uri,
                                mbid: entry.mbid.clone(),
                                span_ms,
                                furthest_ms: 0,
                                heard_ms: 0,
                                seen_at_ms: None,
                                span_honoured: false,
                                was_current: false,
                            },
                        );
                        self.queue_len += 1;
                        return;
                    }
                    eprintln!("addid returned no Id for cue track {uri}");
                }
                Err(e) => eprintln!("mpd addid {uri}: {e}"),
            }
            // A cue track that would not add is not a reason to give up on the
            // passage: the file underneath it still plays, just unnamed.
        }

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
                        span_ms,
                        furthest_ms: 0,
            heard_ms: 0,
            seen_at_ms: None,
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
        in_queue_order(&self.order, &self.ours, &self.names)
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

    /// Begin the **first** queued song at this offset.
    ///
    /// First in MPD's order, not first out of a hash map. `ours.keys().next()`
    /// picked an arbitrary one of them, so a handoff could seek — and so start
    /// — a passage from the middle of the queue rather than the one being
    /// handed over.
    ///
    /// `order` is appended to by `enqueue` rather than waiting for the next
    /// poll, because a handoff enqueues and seeks in the same breath: read
    /// from a poll-stale order this found nothing, sent no seek, and left the
    /// guest holding a full queue and not playing `[SPEC-BK-065]`.
    fn resume_at(&mut self, position_ms: u64) {
        let head_of_ours = |b: &Self| {
            b.order.iter().find(|(id, _)| b.ours.contains_key(id)).map(|(id, _)| id.clone())
        };
        // A handoff enqueues and resumes in the same breath, so the order read
        // at the last poll predates everything just added and matches nothing.
        // Read it again rather than seek blindly: sending no seek left the guest
        // holding a full queue and silent, which is what this looked like on the
        // appliance `[SPEC-BK-065]`.
        let first = head_of_ours(self).or_else(|| {
            self.poll();
            head_of_ours(self)
        });
        if let Some(id) = first {
            let _ = self.mpd.cmd(&format!("seekid {id} {:.3}", position_ms as f64 / 1000.0));
        }
    }

    /// Move inside the song MPD is playing now `[REQ-VIS-225]`.
    ///
    /// **Clamped, because MPD stops a song seeked past its span** — measured:
    /// `seekid 70` into a 60-second range returned `state=stop` rather than
    /// landing at the end `[SPEC-BK-055]`. The offset is already span-relative
    /// on both sides, so it crosses unaltered.
    ///
    /// Addressed to the song that is *current*, not to the first this backend
    /// offered: the listener may be on something they queued themselves
    /// `[SPEC-MPD-115]`, and seeking the wrong song is worse than not seeking.
    fn seek_to(&mut self, position_ms: u64) {
        let Some((id, span)) = self
            .head
            .and_then(|(passage, _)| {
                self.ours.iter().find(|(_, o)| o.passage_id == passage).map(|(id, o)| (id.clone(), o.span_ms))
            })
        else {
            return;
        };
        let at = position_ms.min(span.saturating_sub(SEEK_MARGIN_MS));
        if self.mpd.cmd(&format!("seekid {id} {:.3}", at as f64 / 1000.0)).is_err() {
            return;
        }
        // The clock has moved without anyone listening to the distance. Reading
        // it back on the next poll would otherwise credit the jump as heard
        // `[SPEC-PLAY-012]`.
        if let Some(o) = self.ours.get_mut(&id) {
            o.seen_at_ms = None;
        }
        self.head = self.head.map(|(passage, _)| (passage, at));
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

    fn offered(passage_id: i64) -> Offered {
        Offered {
            passage_id,
            uri: String::new(),
            mbid: None,
            span_ms: 1_000,
            furthest_ms: 0,
            heard_ms: 0,
            seen_at_ms: None,
            span_honoured: true,
            was_current: false,
        }
    }

    /// **The listener's order, not a hash map's** `[SPEC-BK-030]`.
    ///
    /// What this returns is what crosses a handoff, and `carry_queue` re-enqueues
    /// in the order it is handed. Read straight out of `ours` it came back
    /// shuffled, so moving back to Vaino rearranged what was coming up.
    #[test]
    fn the_queue_crosses_in_the_order_mpd_holds_it() {
        let mut ours = HashMap::new();
        // Deliberately inserted in a different order from the queue's, which is
        // what a map would report and what the bug did report.
        ours.insert("30".to_string(), offered(300));
        ours.insert("10".to_string(), offered(100));
        ours.insert("20".to_string(), offered(200));

        let order = [
            ("10".to_string(), "a.mp3".to_string()),
            ("20".to_string(), "b.mp3".to_string()),
            ("30".to_string(), "c.mp3".to_string()),
        ];

        assert_eq!(in_queue_order(&order, &ours, &HashMap::new()), vec![100, 200, 300]);
    }

    /// **The settled rule `[SPEC-BK-045]`: what cannot be named is dropped, and
    /// the rest goes through in the listener's order.**
    ///
    /// A song Vaino never offered is the listener's own addition. Named where
    /// the library can name it, passed over where it cannot — an entry is
    /// unnameable when its file carries more than one radio passage, and a
    /// whole-file entry could be any of up to forty of them.
    #[test]
    fn what_the_listener_queued_crosses_too_and_what_cannot_be_named_does_not() {
        let mut ours = HashMap::new();
        ours.insert("10".to_string(), offered(100));

        let mut names: HashMap<String, Nameable> = HashMap::new();
        names.insert("mine.mp3".into(), Some((200, None, 1_000)));
        // In the library, but a capture: it could be any of its passages.
        names.insert("capture.mp3".into(), None);

        let order = [
            ("10".to_string(), "ours.mp3".to_string()),
            ("20".to_string(), "mine.mp3".to_string()),
            ("30".to_string(), "capture.mp3".to_string()),
            ("40".to_string(), "unknown.mp3".to_string()),
        ];

        assert_eq!(
            in_queue_order(&order, &ours, &names),
            vec![100, 200],
            "ours, then theirs; the ambiguous and the unknown are left behind"
        );
    }

    /// `playlistinfo` interleaves each song's fields, so the `Id:` after a
    /// `file:` is the one that belongs to it.
    #[test]
    fn the_queue_is_read_as_pairs_in_order() {
        let lines: Vec<String> = [
            "file: a.mp3", "Last-Modified: x", "Time: 100", "Pos: 0", "Id: 7",
            "file: b.mp3", "Time: 200", "Pos: 1", "Id: 9",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        assert_eq!(
            queue_of(&lines),
            vec![
                ("7".to_string(), "a.mp3".to_string()),
                ("9".to_string(), "b.mp3".to_string())
            ]
        );
    }

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
