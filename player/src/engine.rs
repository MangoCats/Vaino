//! The playback engine: the one place that owns queue, decoders, mixer and output.
//!
//! Everything below it is a component with no opinion about time — the fader
//! knows curves, the mixer knows addition, the queue knows ordering. The engine
//! is where they meet, and deliberately the only place, so there is one pump
//! rather than one per binary.
//!
//! [`Engine::tick`] performs exactly one pump iteration and is public, so the
//! whole engine is testable without a thread, an audio device, or real time.

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};

use std::time::{Duration, Instant};

use crate::db::PlayerStore;
use crate::decoder::PassageDecoder;
use crate::fade::{Curve, Fade};
use crate::mixer::{mix, Stream};
use crate::queue::{should_admit, Queue, QueueEntry};
use crate::resample::Resampler;
use crate::BUFFER_FRAMES;

struct Live {
    dec: PassageDecoder,
    stream: Stream,
    resampler: Resampler,
    converted: Vec<f32>,
    entry: QueueEntry,
    frames_mixed: u64,
    /// Where in the passage this decode began. Non-zero only when resuming, so
    /// reported position stays position-WITHIN-THE-PASSAGE rather than
    /// restarting at zero and making the next save move backwards.
    origin_ms: u64,
    /// `gain_db` as a linear factor, applied per passage `[REQ-AUD-130]`.
    /// Per passage rather than at the mix, so each side of a crossfade carries
    /// its own level -- applying it after mixing would level the blend, not the
    /// tracks, and the whole point is that they meet at a matched loudness.
    gain: f32,
}

/// A play already written to history, whose passage finished decoding but may
/// still be sounding out of the output ring `[REQ-VIS-250]`.
///
/// **Why this waits rather than reading `heard_ms` on the spot.** A passage
/// leaves `live` the instant its decoder is exhausted, which is up to a
/// ring's depth -- `BUFFER_FRAMES`, ~15 s here -- before its last sample
/// actually reaches the speaker `[REQ-VIS-240]`. Finalising there froze the
/// figure at "decoded", not "heard": a track played all the way through and
/// left with 15 s of itself still queued behind it read as ~94%, never 100%,
/// however completely it was listened to. `at_ms`/`since` are the same pair
/// `draining` carries, so the estimate advances exactly as the position
/// display already does -- one clock, trusted by both.
struct PendingFinish {
    play_id: i64,
    span_ms: u64,
    at_ms: u64,
    since: Instant,
}

impl PendingFinish {
    /// How much would have been heard by now, capped at the passage's own
    /// length -- the clock must not run past the music.
    fn estimate(&self) -> u64 {
        (self.at_ms + self.since.elapsed().as_millis() as u64).min(self.span_ms)
    }
}

/// What the UI and the persistence layer read. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub playing: bool,
    pub current: Option<QueueEntry>,
    /// Audible position, not mixed position. The output ring holds ~14 s, so
    /// what has been mixed runs well ahead of what is being heard; saving the
    /// mixed figure would resume ~14 s late every time.
    pub position_ms: u64,
    pub queue_len: usize,
    /// What is coming, in play order.
    pub queue: Vec<QueueEntry>,
    /// How many of those the mixer already holds, and so cannot be edited
    /// `[REQ-VIS-185]`. They sit at the front of `queue`.
    pub mixing_ahead: usize,
    /// Master volume, 0.0 to 1.0.
    pub volume: f32,
    /// Skip transition shape `[REQ-AUD-162]`, so a UI can show what it will do.
    pub skip_fade_ms: u64,
    pub skip_lead_ms: u64,
    pub resume_save_ms: u64,
    /// Skip suppression window in hours `[SPEC-PLAY-050]`.
    pub skip_suppress_h: u64,
    /// Queue-removal suppression window in hours `[SPEC-PLAY-055]`.
    pub dequeue_suppress_h: u64,
    /// Passages kept ahead, and how often a guest samples `[SPEC-MPD-105]`.
    pub queue_depth: usize,
    pub sample_interval_ms: u64,
    /// `[REQ-VIS-205]`
    pub cue_sheets: bool,
    /// `[REQ-VIS-210]`
    pub covers: bool,
    /// `[REQ-VIS-215]`
    pub lyrics_cache: bool,
    /// `[REQ-VIS-220]`
    pub lyrics_sidecar: bool,
    /// Reported to the browser so the interface can show it `[PI-SET-016]`.
    pub dev_mode: bool,
    pub active_streams: usize,
    pub underrun_samples: u64,
    /// The count **since the baseline**, which is what the interface shows
    /// `[REQ-VIS-230]`. `underrun_samples` stays cumulative for the life of
    /// the process, because that answers a different question and something
    /// should still be able to ask it.
    pub underruns_since_reset: u64,
    /// When that baseline was taken, as a unix time. The process start until
    /// somebody asks for a new one.
    pub underruns_since: i64,
    /// Times the callback could not take the output lock at all. Distinct from
    /// an underrun because the remedy differs: contention argues for a
    /// lock-free ring, starvation for more buffering. Surfaced rather than
    /// merely counted -- a diagnostic nobody can read is not one.
    pub lock_failures: u64,
    /// Output reopenings after a failure `[IMPL-AUD-020]`.
    pub out_recoveries: u64,
    /// Samples handed to the device but not yet played. Playback is NOT over
    /// while this is non-zero: the output ring holds ~14 s, so a short passage
    /// can be fully submitted before a single sample is audible.
    pub output_buffered: usize,
}

impl PlayerState {
    /// Nothing left to decode, queue, or play. The output check is what stops
    /// a caller exiting mid-passage and truncating the tail.
    pub fn is_idle(&self) -> bool {
        self.active_streams == 0 && self.queue_len == 0 && self.output_buffered == 0
    }
}

/// Playback has exactly **two** states, playing and paused. There is no
/// "stopped": pausing halts only the *consumer*, while decoders keep filling
/// their buffers, so resuming is instant and the pipeline stays primed after
/// the initial power-on fill.
/// Where a batch of passages goes `[REQ-VIS-195]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Front of the queue, then skip into it. The only one that interrupts.
    Now,
    /// After the current passage.
    Next,
    /// Behind everything already waiting.
    Last,
}

#[derive(Debug)]
pub enum Command {
    Play,
    Pause,
    /// Drop the playing passage and start the next immediately.
    Skip,
    /// Master volume, clamped to 0.0..=1.0.
    SetVolume(f32),
    /// How long a skip fades the outgoing passage out, in ms `[REQ-AUD-158]`.
    SetSkipFade(u64),
    /// How long after a skip the next passage starts, in ms `[REQ-AUD-162]`.
    SetSkipLead(u64),
    /// How often the resume point is written, in ms `[REQ-VIS-155]`.
    SetResumeSave(u64),
    /// How long a skipped passage stays out of selection, in hours
    /// `[SPEC-PLAY-050]`. Zero turns suppression off.
    SetSkipSuppress(u64),
    /// The same for a passage removed from the queue unheard
    /// `[SPEC-PLAY-055]`.
    SetDequeueSuppress(u64),
    /// How many passages to keep queued ahead `[SPEC-MPD-105]`.
    SetQueueDepth(usize),
    /// Whether Vaino may write cue sheets into the music folder
    /// `[REQ-VIS-205]`. Turning it on is what asks for them to be written.
    SetCueSheets(bool),
    /// Whether Vaino may write cover art into the music folder `[REQ-VIS-210]`.
    SetCovers(bool),
    /// Whether Vaino may write per-song lyrics into a local client's cache
    /// `[REQ-VIS-215]`. Turning it on is what asks for them to be written.
    SetLyricsCache(bool),
    /// Whether Vaino may write lyrics beside the audio `[REQ-VIS-220]`.
    SetLyricsSidecar(bool),
    /// How often a guest backend samples `status`, in ms `[SPEC-MPD-105]`.
    SetSampleInterval(u64),
    /// Start the underrun count again from here `[REQ-VIS-230]`.
    RestartUnderruns,
    Enqueue(QueueEntry),
    /// Put a passage next rather than last, for a browsed choice
    /// `[REQ-VIS-180]`.
    EnqueueNext(QueueEntry),
    /// Play a passage at once: to the front of the queue, then skip into it.
    PlayNow(QueueEntry),
    /// Several passages at once, in the order given `[REQ-VIS-195]`.
    EnqueueMany(Vec<QueueEntry>, Placement),
    /// Drop a queued passage `[REQ-VIS-185]`.
    RemoveQueued(u64),
    /// Move a queued passage earlier (negative) or later (positive).
    ShiftQueued(u64, isize),
    /// Rebuild the output stream against the current default sink
    /// `[PI3-API-010]`.
    ///
    /// Needed because the ALSA bridge binds a stream to whichever node was
    /// default when it opened: changing the default afterwards does not move an
    /// existing stream, so choosing a speaker in the settings panel is
    /// cosmetic -- it looks like it worked, and is silent -- unless the output
    /// is reopened `[PI3-WHY-020]`.
    ReopenOutput,
    /// A second, independent output ring to feed alongside (never instead
    /// of) `path.ring`, or `None` to stop `[Sonos/SONOS008 §6]`.
    ///
    /// The mixer never knows *why* -- only that something else wants a copy
    /// of the same mixed samples. Setting it does not touch `path.ring` at
    /// all, which is how switching to Sonos output stops the local device
    /// (a separate, existing step: `path.ring` becomes `None` because
    /// nothing opened a local device that session, not because this command
    /// closed one).
    #[cfg(feature = "sonos")]
    SetSonosRing(Option<crate::output::OutputRing>),
    /// Write the resume point NOW, ignoring the save interval `[REQ-VIS-155]`.
    ///
    /// For the moments the interval was not designed for: the machine is about
    /// to be powered off deliberately `[PI5-PWR-010]`, and losing the last few
    /// seconds of position to a timer would be a shame in exactly the case a
    /// person took care over.
    Persist,
    /// Terminate the process. Deliberately NOT a playback state -- it ends the
    /// engine rather than putting playback into a third mode.
    Shutdown,
}

/// The control surface, safe to share across threads.
///
/// `tx` is behind a mutex so the handle is `Sync` and can sit in an `Arc` that
/// every web request holds. An `mpsc::Sender` is `Send` but not `Sync`, and
/// commands arrive at human rates, so the lock is never contended.
pub struct EngineHandle {
    tx: Mutex<Sender<Command>>,
    pub state: Arc<Mutex<PlayerState>>,
}

impl EngineHandle {
    pub fn send(&self, c: Command) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(c);
        }
    }
    pub fn snapshot(&self) -> PlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

pub struct Engine {
    queue: Queue,
    live: Vec<Live>,
    /// The next passage, opened and decoded ahead of need `[REQ-AUD-160]`.
    ///
    /// Held OUTSIDE `live` because `live` is what the mixer sums: a passage in
    /// there is sounding. This one is merely ready, and `top_up_decoders` keeps
    /// it fed so promoting it costs nothing but the move.
    ready: Option<Live>,
    /// Room in the output ring as of the last submit. See `mix_and_submit`.
    out_room: usize,
    /// When the shared snapshot may next be written. See `publish`.
    publish_at: Option<std::time::Instant>,
    /// The audible passage as last published, so a change can bypass the clock.
    published: Option<i64>,
    /// Misses already reported, so each is logged once.
    last_lock_failures: u64,
    /// The audio path, held at arm's length `[SPEC-APS-070]`.
    ///
    /// A ring to write into and a channel to ask things of -- and deliberately
    /// nothing that can open a device, wait for one, or ask the system about
    /// one. The device's whole lifecycle belongs to the supervisor, on its own
    /// thread, because every time this loop was allowed to do that work it
    /// eventually did some of it blocking `[GDE-FBD-090]`.
    path: crate::path::PathHandle,
    /// A second output the same mixed samples are also handed to, whenever
    /// one is chosen `[Sonos/SONOS008 §6]`. `None` the overwhelming majority
    /// of the time -- Sonos output is a listener's deliberate choice, not a
    /// default -- and costs one extra `submit` when it is not.
    #[cfg(feature = "sonos")]
    sonos_ring: Option<crate::output::OutputRing>,
    out_rate: u32,
    out_channels: usize,
    scratch: Vec<f32>,
    state: Arc<Mutex<PlayerState>>,
    rx: Receiver<Command>,
    playing: bool,
    shutdown: bool,
    volume: f32,
    /// Skip transition shape, adjustable while playing `[REQ-AUD-162]`.
    skip_fade_ms: u64,
    skip_lead_ms: u64,
    store: Option<PlayerStore>,
    /// One-shot: the offset the NEXT admitted passage opens at. Consumed on
    /// use, so only the resumed passage is seeked and everything after it
    /// starts where the library says it starts.
    pending_resume: Option<u64>,
    last_save: Instant,
    /// How often the resume point is written `[REQ-VIS-155]`. Configurable
    /// because every one of these writes lands on the appliance's most
    /// volatile partition `[PI-C-010]`.
    resume_save_ms: u64,
    /// How long a skipped passage stays out of selection `[SPEC-PLAY-050]`.
    skip_suppress_h: u64,
    /// How long a passage removed from the queue unheard stays out
    /// `[SPEC-PLAY-055]`.
    dequeue_suppress_h: u64,
    /// How often a guest backend should sample `status` `[SPEC-MPD-105]`. The
    /// local engine does not poll; it holds the value so one row owns every
    /// listener setting and the settings page has one place to read.
    sample_interval_ms: u64,
    /// Whether Vaino may write cue sheets into the music folder
    /// `[REQ-VIS-205]`. Held here for the same reason: one row, one page.
    cue_sheets: bool,
    /// `[REQ-VIS-210]`
    covers: bool,
    /// `[REQ-VIS-215]`
    lyrics_cache: bool,
    /// `[REQ-VIS-220]`
    lyrics_sidecar: bool,
    /// Set only while a handoff's fade is running, so the departing head is
    /// not written down as a rejection `[SPEC-BK-065]`.
    handing_over: bool,
    /// The passage that has finished mixing but is still being heard: which
    /// one, where it had got to, and when that was `[REQ-VIS-240]`.
    ///
    /// A passage leaves `live` when its decoder is exhausted, which is up to a
    /// ring's depth before its last sample reaches the speaker. Without this
    /// the displayed position simply stopped there — fifteen seconds short of
    /// the end, every track.
    ///
    /// **Advanced by the clock, not by the ring.** The obvious measure — what
    /// it had mixed, less what is still buffered — is wrong here: during a
    /// crossfade the incoming passage is filling that same ring, so its depth
    /// says nothing about how much of the outgoing one is left. What is left
    /// is simply time, and audio is played at one second per second.
    draining: Option<(i64, u64, Instant)>,

    /// A passage arriving mid-play whose play another backend has already
    /// recorded. Judged as recorded the moment it becomes the head, so it
    /// earns neither a second play nor a rejection `[SPEC-BK-065]`.
    counted_elsewhere: Option<i64>,

    /// How much of the sounding passage has actually been **heard**
    /// `[SPEC-PLAY-012]`.
    ///
    /// Not its position. While playback only ever ran forwards the two were
    /// the same number and the position was used directly; a seek separates
    /// them, and using position would let a drag to the last chorus earn a
    /// play nobody listened to.
    heard_ms: u64,
    /// The position at the previous sample, or `None` when there is no
    /// previous position to measure from — a new passage, or the far side of
    /// a seek. Time is credited from the gap between samples, so a `None`
    /// here is what makes a jump cost nothing.
    heard_from: Option<u64>,
    saved: Option<(i64, bool)>,
    /// The last passage written to play history, so a passage is recorded
    /// once however many ticks it sounds for.
    /// Whether the passage at `head` has been written to history yet.
    recorded: bool,
    /// The passage currently at the head of `live`, so `recorded` can reset.
    head: Option<i64>,
    /// Its MBID, kept because a skip is written *after* the passage has gone
    /// and the entry it came from is no longer reachable `[SPEC-PLAY-050]`.
    head_mbid: Option<String>,
    /// The head's passage span, kept for the same reason as `head_mbid`: a
    /// skip's percentage-played is written after the passage has gone, and by
    /// then `live` no longer holds it `[REQ-VIS-250]`.
    head_span_ms: u64,
    /// The row `record_play` wrote for the head, if any -- so the eventual
    /// departure can go back and fill in how much was actually heard, rather
    /// than freezing the figure at the instant the play was earned
    /// `[REQ-VIS-250]`. `None` once there is nothing left to finish: cleared
    /// after use and whenever the head changes.
    pending_play_id: Option<i64>,
    /// A play whose passage exhausted naturally and is still draining
    /// through the ring, waiting for the clock to say how much of the tail
    /// was actually heard `[REQ-VIS-250]`. At most one at a time, the same
    /// simplification `draining` makes for the display.
    pending_finish: Option<PendingFinish>,
    /// Passages chosen but never opened, waiting to be reported `[REQ-PD-112]`.
    ///
    /// The engine drops them; only the Director can undo having counted them,
    /// and it lives on the other side of `Session`. Kept here until asked for.
    dropped: Vec<i64>,
    /// The passage the LISTENER is on, which is not the one being mixed
    /// `[REQ-AUD-164]`. Held here because it outlives `live`: a passage stays
    /// audible for a ring's depth after the mixer has finished with it.
    shown: Option<(QueueEntry, u64)>,
    /// Underruns that happened while PLAYING. Under the two-state model the
    /// device callback drains continuously, so a paused player underruns
    /// forever -- counting those would bury the fault this number exists to
    /// expose [REQ-AUD-142].
    underruns_playing: u64,
    /// What `underruns_playing` read when the count was last restarted, and
    /// when that was `[REQ-VIS-230]`.
    ///
    /// **In memory, never persisted.** The cumulative counter starts at zero
    /// in every process, so a baseline restored from a previous one would be
    /// subtracting a number that no longer exists.
    underrun_baseline: u64,
    underrun_since: i64,
    last_raw_underruns: u64,
}

/// Seconds since the epoch, for stamping when a count was restarted.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Engine {
    /// Least worth mixing in one pass, in samples `[GDE-FBD-090]`.
    ///
    /// ~46 ms at 44.1 kHz stereo, against a ring holding ~14 s. Sized to be
    /// large enough that the pass earns its lock acquisition and small enough
    /// that it is a rounding error against the buffer it feeds.
    const MIN_SUBMIT: usize = 4096;

    /// How often the shared snapshot is rewritten.
    ///
    /// Browsers are pushed to every 500 ms (`web::PUSH_EVERY`), so publishing
    /// on every tick rewrote the state some two hundred times for each read of
    /// it -- and took the output lock to do so. A tenth of a second is well
    /// inside what any consumer can perceive and two orders of magnitude less
    /// work.
    pub(crate) const PUBLISH_EVERY: std::time::Duration =
        std::time::Duration::from_millis(100);

    /// `out` of `None` runs the full pipeline into a discard sink — useful for
    /// tests and headless hosts, but note it reports no device rate and so
    /// cannot catch a resampling fault `[REQ-HW-147]`.
    pub fn new(path: crate::path::PathHandle, min_depth: usize) -> (Self, EngineHandle) {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(PlayerState::default()));
        let out_rate = path.sample_rate();
        let out_channels = path.channels();
        let engine = Self {
            queue: Queue::new(min_depth),
            live: Vec::new(),
            ready: None,
            path,
            #[cfg(feature = "sonos")]
            sonos_ring: None,
            out_room: 0,
            publish_at: None,
            published: None,
            last_lock_failures: 0,
            out_rate,
            out_channels,
            scratch: vec![0.0; 2048 * out_channels],
            state: Arc::clone(&state),
            rx,
            playing: false,
            shutdown: false,
            volume: 1.0,
            skip_fade_ms: crate::SKIP_FADE_MS,
            skip_lead_ms: crate::SKIP_LEAD_MS,
            store: None,
            pending_resume: None,
            last_save: Instant::now(),
            resume_save_ms: crate::RESUME_SAVE_MS,
            skip_suppress_h: crate::SKIP_SUPPRESS_H,
            dequeue_suppress_h: crate::DEQUEUE_SUPPRESS_H,
            sample_interval_ms: crate::SAMPLE_INTERVAL_MS,
            cue_sheets: false,
            covers: false,
            lyrics_cache: false,
            lyrics_sidecar: false,
            handing_over: false,
            counted_elsewhere: None,
            draining: None,
            heard_ms: 0,
            heard_from: None,
            saved: None,
            recorded: false,
            head: None,
            head_mbid: None,
            head_span_ms: 0,
            pending_play_id: None,
            pending_finish: None,
            shown: None,
            dropped: Vec::new(),
            underruns_playing: 0,
            underrun_baseline: 0,
            // Seeded now, so a fresh player reads "since 09:14" rather than
            // "since never" -- and that is the honest label for the count it
            // is already showing.
            underrun_since: unix_now(),
            last_raw_underruns: 0,
        };
        (engine, EngineHandle { tx: Mutex::new(tx), state })
    }

    /// Persist playback state to `vaino.db` `[REQ-AUD-140]`. Optional: without
    /// it the engine runs identically and simply forgets across restarts.
    pub fn attach_store(&mut self, store: PlayerStore) {
        self.store = Some(store);
    }

    /// Open the next admitted passage `position_ms` in, for resuming.
    pub fn resume_at(&mut self, position_ms: u64) {
        self.pending_resume = Some(position_ms);
    }

    pub fn enqueue(&mut self, e: QueueEntry) {
        self.queue.push(e);
    }
    /// What is queued ahead, in play order. The engine's queue is the only
    /// answer to "what plays next"; a caller that drew its own preview from
    /// the library would be describing a different evening.
    pub fn queued(&self) -> impl Iterator<Item = &QueueEntry> {
        self.queue.iter()
    }
    pub fn shortfall(&self) -> usize {
        self.queue.shortfall()
    }
    /// The engine has been told to terminate. Distinct from paused, which is a
    /// playback state and leaves the pipeline running.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown
    }

    /// One pump iteration: commands, admission, decode, mix, submit, publish.
    ///
    /// Everything here is a load, a lock the callback only ever *tries* for, or
    /// a channel send. Nothing opens a device, sleeps, or asks the system a
    /// question -- those belong to the supervisor `[SPEC-APS-070]`, and the
    /// engine no longer holds anything that could do them `[GDE-FBD-090]`.
    pub fn tick(&mut self) -> usize {
        self.drain_commands();
        if self.shutdown {
            return 0;
        }
        self.admit_due();
        // Prepare AFTER admitting, so this readies the passage that is next
        // once the admission has moved the queue on.
        self.prepare_next();
        // Producers run in BOTH states. Pausing stops the consumer only, so
        // buffers stay full and resuming does not re-incur a fill.
        self.top_up_decoders();
        // Submitting while paused would be audible -- the callback drains
        // continuously -- so only the consumer side is gated.
        // Nothing audible means nothing advances. Mixing on into a failed or
        // dummy-bound output consumed the queue at whatever speed the decoders
        // managed, so the clock raced while the room stayed silent -- a player
        // that lies about what it is doing, which is the fault this whole
        // effort exists to remove `[PI3-API-030]`.
        let audible = self.path.audible();
        let submitted = if self.playing && audible { self.mix_and_submit() } else { 0 };
        self.retire_finished();
        self.record_play();
        // Independent of `self.playing`, the same as `advance_shown` below:
        // the ring drains at the device's own pace regardless of pause
        // `[REQ-VIS-250]`.
        self.finalize_draining_plays();
        self.advance_shown();
        // Throttled by time, but never at the cost of a late answer to the
        // question anyone actually asks: a change of audible passage is
        // published the moment it happens, and the clock only governs the
        // position ticking along in between.
        let now = std::time::Instant::now();
        let changed = self.shown.as_ref().map(|(e, _)| e.passage_id) != self.published;
        if changed || self.publish_at.is_none_or(|t| now >= t) {
            self.publish_at = Some(now + Self::PUBLISH_EVERY);
            self.publish();
        }
        self.persist(false);
        submitted
    }

    fn drain_commands(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(Command::Play) => self.set_playing(true),
                Ok(Command::Pause) => self.set_playing(false),
                Ok(Command::ReopenOutput) => self.path.reopen(),
                #[cfg(feature = "sonos")]
                Ok(Command::SetSonosRing(ring)) => {
                    // Exclusivity, enforced here rather than left to the
                    // caller `[Sonos/SONOS010 §1]`: choosing Sonos silences
                    // the local device (the same mechanism pause already
                    // uses -- the device stays attached, only quiet, so a
                    // Bluetooth link survives it `[PI3-OPEN-020]`) without
                    // touching whether the *session* is playing at all.
                    // Restoring local audibility on the way back respects
                    // whatever `self.playing` already was -- a listener who
                    // paused before switching to Sonos does not have local
                    // output resume just because Sonos output stopped.
                    self.path.set_playing(ring.is_none() && self.playing);
                    self.sonos_ring = ring;
                }
                Ok(Command::Skip) => self.skip(),
                Ok(Command::SetSkipFade(ms)) => {
                    self.skip_fade_ms = ms.min(crate::SKIP_FADE_MAX_MS);
                    self.remember_settings();
                }
                Ok(Command::SetResumeSave(ms)) => {
                    self.resume_save_ms =
                        ms.clamp(crate::RESUME_SAVE_MIN_MS, crate::RESUME_SAVE_MAX_MS);
                    self.remember_settings();
                }
                Ok(Command::SetSkipSuppress(h)) => {
                    self.skip_suppress_h =
                        h.clamp(crate::SKIP_SUPPRESS_MIN_H, crate::SKIP_SUPPRESS_MAX_H);
                    self.remember_settings();
                }
                Ok(Command::SetDequeueSuppress(h)) => {
                    self.dequeue_suppress_h =
                        h.clamp(crate::DEQUEUE_SUPPRESS_MIN_H, crate::DEQUEUE_SUPPRESS_MAX_H);
                    self.remember_settings();
                }
                Ok(Command::SetQueueDepth(n)) => {
                    self.queue.min_depth =
                        n.clamp(crate::QUEUE_DEPTH_MIN, crate::QUEUE_DEPTH_MAX);
                    self.remember_settings();
                }
                Ok(Command::SetCueSheets(on)) => {
                    self.cue_sheets = on;
                    self.remember_settings();
                }
                Ok(Command::SetCovers(on)) => {
                    self.covers = on;
                    self.remember_settings();
                }
                Ok(Command::SetLyricsCache(on)) => {
                    self.lyrics_cache = on;
                    self.remember_settings();
                }
                Ok(Command::SetLyricsSidecar(on)) => {
                    self.lyrics_sidecar = on;
                    self.remember_settings();
                }
                Ok(Command::RestartUnderruns) => {
                    // The real counter is untouched; only the mark moves.
                    self.underrun_baseline = self.underruns_playing;
                    self.underrun_since = unix_now();
                }
                Ok(Command::SetSampleInterval(ms)) => {
                    self.sample_interval_ms =
                        ms.clamp(crate::SAMPLE_INTERVAL_MIN_MS, crate::SAMPLE_INTERVAL_MAX_MS);
                    self.remember_settings();
                }
                Ok(Command::SetSkipLead(ms)) => {
                    self.skip_lead_ms =
                        ms.clamp(crate::SKIP_LEAD_MIN_MS, crate::SKIP_LEAD_MAX_MS);
                    self.remember_settings();
                }
                Ok(Command::SetVolume(v)) => {
                    self.volume = v.clamp(0.0, 1.0);
                    // Straight to the device: the callback applies it, so the
                    // change is heard now rather than a ring-depth later.
                    if let Some(r) = &self.path.ring {
                        r.volume.set(self.volume);
                    }
                    self.remember_settings();
                }
                Ok(Command::Enqueue(e)) => self.queue.push(e),
                Ok(Command::EnqueueNext(e)) => self.queue.push_front(e),
                Ok(Command::EnqueueMany(entries, place)) => {
                    if !entries.is_empty() {
                        match place {
                            Placement::Now => {
                                self.queue.insert_at(0, entries);
                                self.skip();
                            }
                            // Position ONE of the queue, which is the top of
                            // "Coming up". The queue holds only what is still
                            // to come -- the sounding passage lives in `live`
                            // and is not in it -- so index 0 is already after
                            // the current one. Inserting at 1 to "leave what is
                            // playing alone" put everything one place too late.
                            Placement::Next => self.queue.insert_at(0, entries),
                            Placement::Last => {
                                for e in entries {
                                    self.queue.push(e);
                                }
                            }
                        }
                    }
                }
                Ok(Command::PlayNow(e)) => {
                    // Front, then skip: skip takes the front of the queue, so
                    // anything less than the front would play the passage that
                    // was already next instead.
                    self.queue.push_front(e);
                    self.skip();
                }
                Ok(Command::RemoveQueued(id)) => {
                    // Taken out by hand before it ever played: a weaker
                    // statement than a skip, and it earns the shorter window
                    // `[SPEC-PLAY-055]`. Read before the removal, since after it
                    // there is nothing left to name.
                    //
                    // Deliberately NOT the same path as a passage the engine
                    // could not open: that is a failure, not a preference, and
                    // `[REQ-PD-112]` requires it leave no mark at all.
                    let declined = self
                        .queue
                        .iter()
                        .find(|e| e.qid == id)
                        .map(|e| (e.passage_id, e.mbid.clone()));
                    if self.queue.remove(id) {
                        if let Some((passage_id, mbid)) = declined {
                            self.note_rejection(
                                crate::db::Rejection::Dequeue,
                                passage_id,
                                mbid.as_deref(),
                                None,
                                None,
                            );
                        }
                    }
                }
                Ok(Command::ShiftQueued(id, delta)) => {
                    self.queue.shift(id, delta);
                }
                Ok(Command::Persist) => self.persist(true),
                Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                    self.shutdown = true;
                    return;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
    }

    /// Stopping the consumer means stopping the DEVICE, not just declining to
    /// submit: the output ring would otherwise play on for its full depth.
    /// Producers are untouched, so the buffers stay primed [REQ-AUD-142].
    fn set_playing(&mut self, on: bool) {
        self.playing = on;
        self.path.set_playing(on);
    }

    /// Begin playing because the saved state said so `[PI5-PWR-030]`.
    ///
    /// Public where `set_playing` is not, and named for the occasion rather
    /// than the mechanism: this is the one caller that is not a person pressing
    /// something, and reading `engine.play_on_resume()` at the call site says
    /// why it is happening where `engine.set_playing(true)` would not.
    pub fn play_on_resume(&mut self) {
        self.set_playing(true);
    }

    /// Fade the sounding passage out and cross into the next `[REQ-AUD-162]`.
    ///
    /// Dropping the passage here is not enough on its own, and the comment that
    /// used to sit on this function claiming otherwise was wrong: it discards
    /// the *decoder's* buffer, but the output ring still holds every sample
    /// already mixed. Measured, that was **14.0 s** from button to new music.
    ///
    /// So the ring is cut to the length of the fade, and the next passage --
    /// already decoded `[REQ-AUD-160]` -- is summed over its tail. The listener
    /// hears the outgoing passage fall away over `skip_fade_ms` while the
    /// incoming one rises from `skip_lead_ms`, the two overlapping for the
    /// difference.
    fn skip(&mut self) {
        if self.live.is_empty() {
            return;
        }
        let ch = self.out_channels.max(1);
        let rate = self.out_rate as u64;
        let fade_samples = (self.skip_fade_ms * rate / 1000) as usize * ch;
        let lead_samples = (self.skip_lead_ms * rate / 1000) as usize * ch;

        // Everything sounding is already mixed into the ring and will be faded
        // there, together. Nothing upstream is worth keeping -- including a
        // passage part-way through an ordinary crossfade, which the listener
        // has barely heard, its decode having run a ring's depth ahead.
        self.live.clear();
        // Promote the prepared passage. Without one this degrades to a plain
        // fade to silence, which is the right answer when the queue is empty.
        self.admit_due();
        // Skip cuts the ring to the fade, so the incoming passage is audible
        // within a second rather than a ring's depth. Handing the display over
        // now keeps the button honest [REQ-AUD-164].
        self.shown = self.live.first().map(|l| (l.entry.clone(), 0));

        self.cut_ring_to_incoming(fade_samples, lead_samples);
    }

    /// Cut the ring back to the fade and overlay whatever is sounding now.
    ///
    /// **The reason a skip is heard within a second** rather than after a ring's
    /// depth — about 14 s of already-mixed audio otherwise stands between the
    /// listener and the change they asked for. Lifted out of `skip` when `seek`
    /// turned out to need exactly it: both replace what is in the ring with a
    /// different point in the music, and differ only in which point.
    fn cut_ring_to_incoming(&mut self, fade_samples: usize, lead_samples: usize) {
        // Whatever a previously-departed passage still had draining through
        // this ring is about to be wiped along with everything else in it --
        // take its estimate as final now, rather than let a skip or a seek
        // strand it waiting for a tail that will never finish arriving
        // `[REQ-VIS-250]`.
        self.resolve_pending_finish_now();
        // How much of the outgoing survives the cut sets how much of the
        // incoming overlaps it. Asked before the cut, since afterwards the
        // answer is by definition the fade length.
        let have = self.path.ring.as_ref().map_or(0, |r| r.buffered()).min(fade_samples);
        let wanted = have.saturating_sub(lead_samples);

        // **Fill the incoming stream before cutting the ring** `[PI-CHR-075]`.
        //
        // The overlay is mixed from whatever the incoming passage has already
        // decoded. `prepare_next` opens it early precisely so there is
        // something there — but opening is not decoding, and `top_up` fills
        // it over the ticks that follow. A skip landing in that window found
        // 882 samples where it wanted 132,300, laid almost nothing into the
        // ring it had just cut, and left about a second of silence
        // `[PI-CHR-075]`. Measured on the appliance, where seeking into a
        // 244-minute capture on an SD card holds that window open for
        // seconds; a desktop closes it too fast to notice.
        //
        // Bounded, because this runs where the listener is waiting: enough
        // attempts to cover the overlay and no more. Falling short is not a
        // failure — it degrades to the old behaviour rather than blocking.
        if let Some(l) = self.live.first_mut() {
            for _ in 0..crate::TOPUP_TRIES_BEFORE_CUT {
                if l.stream.ring.len() >= wanted || l.stream.finished {
                    break;
                }
                Self::top_up(l);
            }
        }

        let mut overlay = vec![0.0f32; wanted];
        if let Some(l) = self.live.first_mut() {
            // Through `mix`, not by reading the ring directly: the fade-in has
            // been applied on the way in `[XFD-ORTH-020]`, and this keeps the
            // accounting identical to an ordinary tick.
            let before = l.stream.ring.len();
            let filled = mix(std::iter::once(&mut l.stream), &mut overlay);
            let consumed = before.saturating_sub(l.stream.ring.len());
            l.frames_mixed += (consumed / l.stream.channels.max(1)) as u64;
            overlay.truncate(filled);
        }

        if let Some(o) = &self.path.ring {
            o.begin_skip_transition(
                self.skip_fade_ms,
                self.skip_lead_ms,
                Curve::Exponential,
                &overlay,
            );
        }
    }

    /// Move to a point inside the passage that is sounding `[REQ-VIS-225]`.
    ///
    /// **The same operation as `skip`, aimed at the same passage instead of the
    /// next one**: clear what is sounding, open at the new point, cut the ring
    /// back so the move is heard at once. Reusing that path rather than writing
    /// a second one keeps a single place where this engine stops sounding one
    /// thing and starts sounding another.
    ///
    /// **It lands alone.** Mid-crossfade, both passages go and the sought one
    /// returns by itself — a seek into a passage that is still fading up under
    /// another would otherwise resume an overlap the listener has left behind.
    ///
    /// The jump itself is not listening `[SPEC-PLAY-012]`: `heard_from` is
    /// cleared so no part of the distance travelled is credited as heard.
    pub fn seek_to(&mut self, position_ms: u64) {
        let Some(head) = self.live.first() else { return };
        let entry = head.entry.clone();
        // A seek to the very end would open a decoder with nothing to decode.
        let at = position_ms.min(entry.duration_ms().saturating_sub(1));
        let opened = match self.open(&entry, at) {
            Ok(l) => l,
            // A file that will not re-open is not a reason to stop the music
            // that is already sounding from it.
            Err(e) => {
                eprintln!("seek in {}: {e}", entry.path.display());
                return;
            }
        };
        let ch = self.out_channels.max(1);
        let rate = self.out_rate as u64;
        let fade_samples = (self.skip_fade_ms * rate / 1000) as usize * ch;
        let lead_samples = (self.skip_lead_ms * rate / 1000) as usize * ch;

        self.live.clear();
        self.live.push(opened);
        // The listener is at the new point the moment they ask, not a ring's
        // depth later `[REQ-AUD-164]`.
        self.shown = self.live.first().map(|l| (l.entry.clone(), at));
        self.heard_from = None;
        self.cut_ring_to_incoming(fade_samples, lead_samples);
    }

    /// The passage sounding and how far into its span, for a handoff that must
    /// not restart it `[SPEC-BK-065]`.
    ///
    /// **The audible position, not the decoded one.** `audible_ms` subtracts
    /// what is sitting in the output ring; `played_ms` would be ahead by the
    /// buffer depth, and handing that over would start the other side a ring's
    /// worth into the future — about 14 s here, which is not a seam but a jump.
    pub fn head_position(&self) -> Option<(i64, u64)> {
        self.live.first().map(|l| (l.entry.passage_id, self.audible_ms(l)))
    }

    /// Whether the sounding passage's play is already in the history
    /// `[SPEC-BK-065]`.
    pub fn head_counted(&self) -> bool {
        self.recorded && !self.live.is_empty()
    }

    /// Adopt a passage another backend already counted, so this one will not
    /// count it again when it arrives `[SPEC-BK-065]`.
    pub fn adopt_counted(&mut self, passage_id: i64) {
        self.counted_elsewhere = Some(passage_id);
    }

    /// Fade out because the passage is being handed to another backend, which
    /// is **not** the listener declining it `[SPEC-BK-065]`.
    ///
    /// Same fade, different meaning. `fade_to_silence` goes through `skip`, and
    /// a skip that leaves before the threshold earns a suppression window
    /// `[SPEC-PLAY-050]` — 156 hours by default. A passage that is still
    /// playing on the other side has not been declined, and suppressing it
    /// would punish the listener for changing rooms.
    ///
    /// **A latch, not a flag around the call.** The fade is asked for here and
    /// the head does not depart until it completes, several ticks later; a flag
    /// cleared on the way out of this function would already be false by the
    /// time the departure was judged. It is set only when something is really
    /// sounding, so it cannot sit armed and swallow the next genuine skip.
    pub fn hand_off_to_silence(&mut self, ms: u64) -> bool {
        self.handing_over = !self.live.is_empty();
        self.fade_to_silence(ms)
    }

    /// Fade what is sounding down to silence and stop `[SPEC-BK-030]`.
    ///
    /// **This is `skip` with nothing to skip to**, which the skip path already
    /// handles: emptying the queue first means `admit_due` promotes nothing, and
    /// the transition it begins has no incoming audio to overlay — so the ring
    /// fades out and stays out. `skip`'s own comment said as much long before a
    /// handoff wanted it.
    ///
    /// Reusing it rather than writing a second fade is the point. There is one
    /// place in this engine that takes the ring from sounding to not, and a
    /// handoff has no business inventing another with its own idea of a curve.
    ///
    /// Returns whether an output was actually faded. With no ring — a silent
    /// path, a failed device — there is nothing to fade and saying otherwise
    /// would be the lie `[PI3-API-030]` refuses.
    pub fn fade_to_silence(&mut self, ms: u64) -> bool {
        // The queue goes first: the passages are already being rebuilt on the
        // other side, and one left here would be promoted into the fade.
        self.queue.clear();
        let had_output = self.path.ring.is_some() && !self.live.is_empty();
        let saved = self.skip_fade_ms;
        self.skip_fade_ms = ms.min(crate::SKIP_FADE_MAX_MS);
        self.skip();
        self.skip_fade_ms = saved;
        had_output
    }

    /// Start the next passage when the current one reaches its lead-out point.
    ///
    /// Position-driven via the shared rule, never buffer-driven `[XFD-BEH-C1-020]`.
    fn admit_due(&mut self) {
        let due = match (self.queue.peek(), self.live.last()) {
            (Some(next), Some(l)) => {
                should_admit(&l.entry, self.played_ms(l), next)
            }
            (Some(_), None) => true,
            _ => false,
        };
        if !due {
            return;
        }
        // The queue holds only UPCOMING passages; admission moves one into
        // `live`, where `live[0]` is what is sounding. Keeping a passage in
        // both places would mean two answers to "what is playing".
        let Some(entry) = self.queue.advance() else { return };
        let origin = self.pending_resume.take();

        // The prepared passage is the queue head already opened at its start,
        // so it serves unless a resume offset overrides where to begin.
        if origin.is_none() {
            if let Some(l) = self.ready.take().filter(|l| l.entry.passage_id == entry.passage_id) {
                self.live.push(l);
                return;
            }
        }
        match self.open(&entry, origin.unwrap_or(0)) {
            Ok(l) => self.live.push(l),
            Err(e) => {
                eprintln!("skipping {}: {e}", entry.path.display());
                self.dropped.push(entry.passage_id);
            }
        }
    }

    /// Write the settings down, now rather than on a timer.
    ///
    /// They change when a hand moves a control, which is rare and deliberate,
    /// and a setting that survives everything except the crash that happens
    /// before the next tick is not really saved. Best-effort: failing to
    /// record a volume must never interrupt the music.
    fn remember_settings(&self) {
        if let Some(store) = &self.store {
            if let Err(e) = store.save_settings(&self.settings()) {
                eprintln!("save settings: {e}");
            }
        }
    }

    /// The listener's settings as the engine currently holds them.
    pub fn settings(&self) -> crate::db::Settings {
        crate::db::Settings {
            volume: self.volume,
            skip_fade_ms: self.skip_fade_ms,
            skip_lead_ms: self.skip_lead_ms,
            resume_save_ms: self.resume_save_ms,
            skip_suppress_h: self.skip_suppress_h,
            dequeue_suppress_h: self.dequeue_suppress_h,
            queue_depth: self.queue.min_depth,
            sample_interval_ms: self.sample_interval_ms,
            cue_sheets: self.cue_sheets,
            covers: self.covers,
            lyrics_cache: self.lyrics_cache,
            lyrics_sidecar: self.lyrics_sidecar,
        }
    }

    /// Put back what was last chosen. Clamped on the way in, because a value
    /// from disk deserves no more trust than one from the network.
    pub fn apply_settings(&mut self, s: &crate::db::Settings) {
        self.resume_save_ms =
            s.resume_save_ms.clamp(crate::RESUME_SAVE_MIN_MS, crate::RESUME_SAVE_MAX_MS);
        self.skip_suppress_h =
            s.skip_suppress_h.clamp(crate::SKIP_SUPPRESS_MIN_H, crate::SKIP_SUPPRESS_MAX_H);
        self.dequeue_suppress_h = s
            .dequeue_suppress_h
            .clamp(crate::DEQUEUE_SUPPRESS_MIN_H, crate::DEQUEUE_SUPPRESS_MAX_H);
        // The queue depth is a listener setting now, not a launch flag
        // `[SPEC-MPD-105]`, and it governs this engine as much as the MPD one.
        self.queue.min_depth =
            s.queue_depth.clamp(crate::QUEUE_DEPTH_MIN, crate::QUEUE_DEPTH_MAX);
        self.sample_interval_ms = s
            .sample_interval_ms
            .clamp(crate::SAMPLE_INTERVAL_MIN_MS, crate::SAMPLE_INTERVAL_MAX_MS);
        self.cue_sheets = s.cue_sheets;
        self.covers = s.covers;
        self.lyrics_cache = s.lyrics_cache;
        self.lyrics_sidecar = s.lyrics_sidecar;
        self.volume = s.volume.clamp(0.0, 1.0);
        if let Some(r) = &self.path.ring {
            r.volume.set(self.volume);
        }
        self.skip_fade_ms = s.skip_fade_ms.min(crate::SKIP_FADE_MAX_MS);
        self.skip_lead_ms =
            s.skip_lead_ms.clamp(crate::SKIP_LEAD_MIN_MS, crate::SKIP_LEAD_MAX_MS);
    }

    /// Passages that were chosen but could not be opened, taken once.
    ///
    /// Draining rather than reading: each is reported to the Director exactly
    /// once, and a second report would restore a rotation entry twice.
    pub fn take_dropped(&mut self) -> Vec<i64> {
        std::mem::take(&mut self.dropped)
    }

    /// Open the passage after this one before anyone asks for it
    /// `[REQ-AUD-160]`.
    ///
    /// Skip used to pay for a file open, a seek and a resampler build at the
    /// moment the button was pressed, and the fade had to be long enough to
    /// hide all of it. Doing that work early is what lets the fade be as short
    /// as it sounds right rather than as long as the decoder needs.
    fn prepare_next(&mut self) {
        // A pending resume owns the next admission and opens at its own offset,
        // so preparing one at the start would be wasted and then discarded.
        if self.pending_resume.is_some() {
            return;
        }
        let Some(next) = self.queue.peek() else { return };
        if self.ready.as_ref().map(|l| l.entry.passage_id) == Some(next.passage_id) {
            return; // already standing by
        }
        let entry = next.clone();
        match self.open(&entry, 0) {
            Ok(l) => self.ready = Some(l),
            Err(e) => {
                // Dropped rather than left in place: retrying an unopenable
                // passage every tick would spin forever and never reach the
                // playable one behind it.
                eprintln!("skipping {}: {e}", entry.path.display());
                self.queue.advance();
                self.dropped.push(entry.passage_id);
                self.ready = None;
            }
        }
    }

    fn open(&self, e: &QueueEntry, origin_ms: u64) -> Result<Live, String> {
        // Clamp: a resume point past the end (the passage was re-trimmed since)
        // must replay the passage, not decode nothing.
        let origin_ms = origin_ms.min(e.duration_ms().saturating_sub(1));
        let dec = PassageDecoder::open(&e.path, e.start_ms + origin_ms, Some(e.end_ms))
            .map_err(|err| err.to_string())?;
        let ch = dec.channels;
        let resampler = Resampler::new(dec.sample_rate, self.out_rate, ch)?;
        // Fades are measured in OUTPUT frames because they are applied after
        // conversion; using the file rate here would mis-time every fade on a
        // device that does not match the file.
        let sr = self.out_rate as f32;
        let fade = Fade {
            curve: Curve::Exponential,
            frames: (e.lead_in_ms as f32 * sr / 1000.0) as u64,
            fade_in: true,
        };
        Ok(Live {
            stream: Stream::new(BUFFER_FRAMES * ch, ch, fade),
            dec,
            resampler,
            converted: Vec::new(),
            gain: 10f32.powf(e.gain_db / 20.0),
            entry: e.clone(),
            frames_mixed: 0,
            origin_ms,
        })
    }

    /// Feeds what is sounding AND what is merely ready, so the prepared passage
    /// arrives at the mixer already full `[REQ-AUD-160]`.
    fn top_up_decoders(&mut self) {
        for l in self.live.iter_mut().chain(self.ready.iter_mut()) {
            Self::top_up(l);
        }
    }

    fn top_up(l: &mut Live) {
        {
            if l.stream.finished
                || l.stream.ring.free() < crate::DECODE_TOPUP_FRAMES * l.stream.channels
            {
                return;
            }
            match l.dec.next() {
                Ok(Some(chunk)) => {
                    l.converted.clear();
                    let mut buf = std::mem::take(&mut l.converted);
                    match l.resampler.process(chunk, &mut buf) {
                        Ok(()) => {
                            if l.gain != 1.0 {
                                for s in buf.iter_mut() {
                                    *s *= l.gain;
                                }
                            }
                            l.stream.push(&mut buf);
                        }
                        Err(e) => {
                            eprintln!("resample: {e}");
                            l.stream.finished = true;
                        }
                    }
                    l.converted = buf;
                }
                Ok(None) => l.stream.finished = true,
                Err(e) => {
                    eprintln!("decode: {e}");
                    l.stream.finished = true;
                }
            }
        }
    }

    /// Mix at most what the output can accept, then submit it.
    ///
    /// Sizing the mix to the free space is not an optimisation, it is
    /// correctness: `mix` CONSUMES from the stream rings, so mixing more than
    /// the output will take discards the surplus permanently. That silently
    /// dropped most of a passage and made nine minutes of audio "finish" in
    /// 66 seconds — inaudible on a null sink, which always accepts everything.
    ///
    /// Limiting here is also what propagates back-pressure: the stream rings
    /// stay full, so the decoders stop, and the device paces the whole chain.
    fn mix_and_submit(&mut self) -> usize {
        // Room is remembered from the last submit rather than asked for again.
        // Between then and now the callback only ever DRAINS, so the remembered
        // figure is a lower bound on what is free and writing it always fits --
        // which is what keeps the assertion below honest. Asking cost a second
        // lock acquisition on every pass, against a callback that must never
        // wait for one.
        let room = match &self.path.ring {
            Some(o) => {
                // Refreshed whenever the remembered figure is too small to act
                // on. It only ever GROWS between submits, so a stale value that
                // is already large enough needs no confirmation -- but one
                // below the threshold must be re-read, or a ring that filled up
                // once would never be topped up again.
                if self.out_room < Self::MIN_SUBMIT { self.out_room = o.free(); }
                self.out_room
            }
            None => self.scratch.len(),
        };
        // Whole frames only; a partial frame would offset every later sample.
        let want = room.min(self.scratch.len()) / self.out_channels * self.out_channels;
        // Don't wake the whole chain to move a handful of samples. The ring
        // holds ~14 s, so there is nothing to gain by topping it up the instant
        // a few samples drain, and a great deal to lose: mixing whatever had
        // appeared since the last pass meant hundreds of passes a second, each
        // taking the output lock, on a machine with four slow cores. The
        // decoders already pace themselves this way `[DECODE_TOPUP_FRAMES]`.
        if want == 0 || (want < Self::MIN_SUBMIT && self.path.ring.is_some()) {
            return 0;
        }

        let before: Vec<usize> = self.live.iter().map(|l| l.stream.ring.len()).collect();
        let filled = mix(
            self.live.iter_mut().map(|l| &mut l.stream),
            &mut self.scratch[..want],
        );
        for (l, was) in self.live.iter_mut().zip(before) {
            let consumed = was.saturating_sub(l.stream.ring.len());
            l.frames_mixed += (consumed / l.stream.channels.max(1)) as u64;
        }
        if filled == 0 {
            return 0;
        }
        // Handed to a second ring, if one is chosen, before anything checks
        // whether the local device even exists `[Sonos/SONOS008 §6]`. The
        // mixer does not know or care who is on the other end -- only that
        // these are the same samples `path.ring` is about to receive.
        //
        // **That independence is not yet real**, found against the real
        // Office pair `[Sonos/SONOS012 §3]`: `filled` above is sized by
        // `path.ring.free()` alone (see `room`, above), so a local device
        // that is `released()` -- mid-reopen, or endlessly re-hunting a
        // Bluetooth speaker that is simply off -- reports zero free space,
        // `mix_and_submit` returns `0`, and *nothing gets mixed for anyone*,
        // Sonos included, for as long as that lasts. Local pacing the whole
        // chain is deliberate and correct when local is the thing draining
        // it `[REQ-AUD-142]`; it was never meant to also gate a completely
        // different, currently-chosen output. Not fixed here -- see
        // `[Sonos/SONOS012 §3]` for the proposed decoupling, held for review
        // before it touches this path.
        #[cfg(feature = "sonos")]
        if let Some(r) = &self.sonos_ring {
            r.submit(&self.scratch[..filled]);
        }
        match &self.path.ring {
            Some(o) => {
                let (taken, free_after) = o.submit(&self.scratch[..filled]);
                debug_assert_eq!(taken, filled, "output accepted less than it reported free");
                self.out_room = free_after;
                taken
            }
            None => filled, // discard sink: report accepted so callers advance
        }
    }

    fn retire_finished(&mut self) {
        // **What it had mixed as it left** `[REQ-VIS-240]`. The listener has
        // not heard the last of it -- a ring's depth of it is still queued for
        // the device -- and `advance_shown` needs this to keep the clock
        // moving over that window.
        for l in self.live.iter().filter(|l| l.stream.is_exhausted()) {
            // The audible position at the moment it stopped being mixed, and
            // the moment itself. Everything after this is arithmetic on the
            // clock.
            self.draining = Some((l.entry.passage_id, self.audible_ms(l), Instant::now()));
        }
        self.live.retain(|l| !l.stream.is_exhausted());
    }

    /// Position within the passage of the audio that has been MIXED. Drives
    /// crossfade admission, which must lead what is heard by the buffer depth.
    fn played_ms(&self, l: &Live) -> u64 {
        l.origin_ms + l.frames_mixed * 1000 / self.out_rate.max(1) as u64
    }

    /// Position within the passage of the audio being HEARD: mixed, less what
    /// is still sitting in the output ring. This is what the UI shows and what
    /// a resume point must record.
    fn audible_ms(&self, l: &Live) -> u64 {
        let frames = self.out_buffered_frames() as u64;
        self.played_ms(l)
            .saturating_sub(frames * 1000 / self.out_rate.max(1) as u64)
    }

    fn out_buffered_frames(&self) -> usize {
        self.path.ring.as_ref().map(|r| r.buffered() / self.out_channels.max(1)).unwrap_or(0)
    }

    /// Write the resume point. Throttled, because a tick is sub-millisecond and
    /// an SQLite write per tick would dominate the loop; `force` bypasses it for
    /// shutdown. Writes immediately when the passage or play state changes, so
    /// the interesting transitions are never the ones lost to a power cut.
    fn persist(&mut self, force: bool) {
        let Some(store) = &self.store else { return };
        let key = (self.live.first().map(|l| l.entry.passage_id).unwrap_or(-1), self.playing);
        let changed = self.saved != Some(key);
        let every = Duration::from_millis(self.resume_save_ms);
        if !force && !changed && self.last_save.elapsed() < every {
            return;
        }
        let (id, pos) = match self.live.first() {
            Some(l) => (Some(l.entry.passage_id), self.audible_ms(l)),
            None => (None, 0),
        };
        if let Err(e) = store.save(id, pos, self.playing) {
            eprintln!("save player state: {e}");
        }
        self.last_save = Instant::now();
        self.saved = Some(key);
    }

    /// Write a play to history once enough of the passage has been heard
    /// `[SPEC-PLAY-010]`, `[SPEC-PLAY-030]`.
    ///
    /// **Not at the start of playback.** *(Changed 2026-08-21.)* This used to
    /// write the moment a passage began sounding, following MuLibPlay, whose
    /// note says history updates "as each new track finishes playing (or is put
    /// in the play queue)" — and it argued that a track skipped after ten
    /// seconds had been *encountered*, so suppressing it was wanted.
    ///
    /// That is now a **measured divergence from MuLibPlay** `[GDE-PHS-030]`.
    /// The threshold is half the passage or four minutes, the same rule the MPD
    /// path judges by and the same one Last.fm and ListenBrainz use, because
    /// both paths write this one table and it cannot mean two things
    /// `[SPEC-PLAY-030]`.
    ///
    /// Measured against **audible** position, net of output buffering: what the
    /// listener heard, not what the decoder reached.
    ///
    /// A failure here must never interrupt playback: history is what the next
    /// selection reads, not what this one depends on.
    fn record_play(&mut self) {
        if !self.playing {
            return;
        }
        // Everything needed is read off the head first, so the borrow ends
        // before any of the bookkeeping below wants `&mut self`.
        //
        // **Read even when there is no head.** An empty `live` is not "nothing
        // to do": it is the strongest evidence a passage has just departed. An
        // earlier version returned here, so skipping the *last* queued passage
        // judged nothing at all — the track was abandoned and suppressed
        // nothing, and the Director could offer it straight back
        // `[SPEC-PLAY-050]`.
        let head_now: Option<(i64, Option<String>, u64, u64)> = self.live.first().map(|live| {
            (
                live.entry.passage_id,
                live.entry.mbid.clone(),
                self.audible_ms(live),
                live.entry.duration_ms(),
            )
        });
        let id_now = head_now.as_ref().map(|(id, ..)| *id);

        // The guard has to follow the head, not just remember the last write.
        // While every started passage was recorded these were the same thing;
        // now that a passage can finish unrecorded, a stale id would suppress
        // the next honest play of the same passage.
        if self.head != id_now {
            // A handoff is a departure without a rejection: the passage did
            // not stop, it moved to the other backend `[SPEC-BK-065]`. Taken
            // rather than read, so it covers exactly one departure.
            let handoff = std::mem::take(&mut self.handing_over);
            if let Some(prev) = self.head {
                if !handoff {
                    if self.recorded {
                        // Already earned a play, and there may be nothing
                        // further to do: a passage adopted mid-play from
                        // another backend, or one whose write itself failed,
                        // leaves no local row to correct.
                        if let Some(play_id) = self.pending_play_id.take() {
                            // If it left because it was CUT SHORT -- a skip,
                            // a seek, anything that did not go through
                            // `retire_finished` -- `heard_ms` is already
                            // final: it was live and tracked right up to the
                            // interruption, no ring left to drain. But if
                            // `draining` names this same passage, it left the
                            // ordinary way -- decoded to its end -- and up to
                            // a ring's depth of it may still be sounding
                            // `[REQ-VIS-250]`. Finalising on the spot there
                            // is exactly the bug this exists to avoid:
                            // freezing the figure at "decoded" rather than
                            // waiting for "heard".
                            match self.draining {
                                Some((id, at, since)) if id == prev => {
                                    self.queue_pending_finish(PendingFinish {
                                        play_id,
                                        span_ms: self.head_span_ms,
                                        at_ms: at,
                                        since,
                                    });
                                }
                                _ => self.write_finish(play_id, self.heard_ms.min(self.head_span_ms)),
                            }
                        }
                    } else {
                        // The outgoing passage left without reaching the
                        // threshold: it did not play, and it is not
                        // forgotten either `[SPEC-PLAY-050]`.
                        let prev_mbid = self.head_mbid.take();
                        self.note_rejection(
                            crate::db::Rejection::Skip,
                            prev,
                            prev_mbid.as_deref(),
                            Some(self.heard_ms),
                            Some(self.head_span_ms),
                        );
                    }
                }
            }
            self.head = id_now;
            self.head_mbid = head_now.as_ref().and_then(|(_, m, ..)| m.clone());
            self.head_span_ms = head_now.as_ref().map(|(.., span)| *span).unwrap_or(0);
            self.pending_play_id = None;
            // A passage that arrives already counted starts its life here as
            // recorded, which is what stops it being counted twice.
            self.recorded = match (id_now, self.counted_elsewhere) {
                (Some(now), Some(already)) if now == already => {
                    self.counted_elsewhere = None;
                    true
                }
                _ => false,
            };
            // A new passage has been heard for none of itself, and there is
            // no earlier position of it to measure the first gap from.
            self.heard_ms = 0;
            self.heard_from = None;
        }

        let Some((id, mbid, position_ms, span_ms)) = head_now else { return };

        // **Credited from the gap between samples, never from the position.**
        // Only forward movement counts, and only movement this sample saw:
        // a seek clears `heard_from`, so the jump across contributes nothing
        // and the next sample simply starts measuring again `[SPEC-PLAY-012]`.
        if let Some(previous) = self.heard_from {
            self.heard_ms += position_ms.saturating_sub(previous);
        }
        self.heard_from = Some(position_ms);

        if self.recorded {
            return;
        }
        if !crate::scrobble::counts_as_play(self.heard_ms, span_ms) {
            return;
        }
        self.recorded = true;
        if let Some(store) = &self.store {
            // `heard_ms` at this instant is only the threshold just crossed --
            // half the passage, or four minutes -- not what will finally have
            // been heard. `finish_play` corrects it once the passage actually
            // departs `[REQ-VIS-250]`.
            match store.record_play(id, mbid.as_deref(), self.heard_ms, span_ms) {
                Ok(play_id) => self.pending_play_id = Some(play_id),
                Err(e) => eprintln!("record play: {e}"),
            }
        }
    }

    /// Correct a play already written with how much was truly heard.
    ///
    /// Best-effort, like the write it corrects: if this never runs -- process
    /// exit, a store error -- the row simply keeps whatever figure it was
    /// last written with, which under-reports rather than over-reports.
    fn write_finish(&self, play_id: i64, heard_ms: u64) {
        let Some(store) = &self.store else { return };
        if let Err(e) = store.finish_play(play_id, heard_ms) {
            eprintln!("finish play: {e}");
        }
    }

    /// Hold a play's correction until the clock says the drain is done
    /// `[REQ-VIS-250]`, replacing whatever was already waiting.
    ///
    /// There is only one slot, the same simplification `draining` itself
    /// makes -- two passages finishing within one ring's depth of each other
    /// is the case neither tracks past. Losing the earlier one silently would
    /// leave its row frozen at the threshold forever, so it is flushed with
    /// its best estimate first rather than dropped.
    fn queue_pending_finish(&mut self, next: PendingFinish) {
        if let Some(prev) = self.pending_finish.take() {
            self.write_finish(prev.play_id, prev.estimate());
        }
        self.pending_finish = Some(next);
    }

    /// Every tick: has a deferred play finished draining on its own?
    /// `[REQ-VIS-250]`. Checked here rather than resolved once and forgotten,
    /// because the answer depends on the clock, not on anything that happens
    /// to run this tick -- the same reason `advance_shown` re-reads `draining`
    /// every time rather than computing it once `[REQ-VIS-240]`.
    fn finalize_draining_plays(&mut self) {
        let Some(pending) = &self.pending_finish else { return };
        let estimate = pending.estimate();
        if estimate >= pending.span_ms {
            let play_id = pending.play_id;
            self.pending_finish = None;
            self.write_finish(play_id, estimate);
        }
    }

    /// A skip or a seek is about to overwrite the ring outright `[REQ-VIS-250]`.
    /// Whatever a still-draining play had reached is as much of it as anyone
    /// will ever hear now, so take the estimate as final rather than let the
    /// interrupted tail count toward it forever.
    fn resolve_pending_finish_now(&mut self) {
        if let Some(pending) = self.pending_finish.take() {
            self.write_finish(pending.play_id, pending.estimate());
        }
    }

    /// How many passages are sounding. For tests that need to know the engine
    /// really went quiet.
    pub fn snapshot_live(&self) -> usize {
        self.live.len()
    }

    /// The suppression windows as the listener has them set, in hours:
    /// `(skip, dequeue)` `[SPEC-PLAY-050]`, `[SPEC-PLAY-055]`.
    pub fn snapshot_suppress_h(&self) -> (u64, u64) {
        (self.skip_suppress_h, self.dequeue_suppress_h)
    }

    /// A passage the listener declined `[SPEC-PLAY-050]`.
    ///
    /// Written to `listener_rejections`, never to `listener_play_history`: it
    /// must not gain a play, a ramp or an artist mark. The only thing it earns
    /// is a spell out of the running.
    ///
    /// Best-effort, like `record_play`. A history write must never interrupt
    /// the music.
    ///
    /// `heard_ms`/`span_ms` are `None` for a dequeue: the passage never
    /// sounded, so there is no percentage to report, not a percentage of
    /// zero `[REQ-VIS-250]`. A skip supplies both -- it always started.
    fn note_rejection(
        &self,
        kind: crate::db::Rejection,
        passage_id: i64,
        mbid: Option<&str>,
        heard_ms: Option<u64>,
        span_ms: Option<u64>,
    ) {
        if let Some(store) = &self.store {
            if let Err(e) = store.record_rejection(kind, passage_id, mbid, heard_ms, span_ms) {
                eprintln!("record {}: {e}", kind.as_str());
            }
        }
    }

    /// Bookkeeping that must keep up with the mixer, separate from writing the
    /// snapshot that anyone reads.
    ///
    /// Split because the two have completely different natural rates: this
    /// tracks what is *audible*, which changes with the ring, while the
    /// snapshot serves browsers polled twice a second.
    fn advance_shown(&mut self) {
        // Attribute the increment before publishing: silence during a pause is
        // expected, silence during playback is the bug worth reporting.
        let (raw, misses) = self.path.ring.as_ref().map(|r| r.diagnostics()).unwrap_or((0, 0));
        let delta = raw.saturating_sub(self.last_raw_underruns);
        self.last_raw_underruns = raw;
        if self.playing {
            self.underruns_playing += delta;
        }
        // Log each one as it happens, so a glitch someone HEARS can be matched
        // against a glitch the player recorded. These were assumed inaudible
        // on the strength of a percentage; they are not, and the way to stop
        // guessing about the remainder is to timestamp them.
        if misses > self.last_lock_failures {
            eprintln!("output: {} missed ring lock(s), {} total",
                      misses - self.last_lock_failures, misses);
            self.last_lock_failures = misses;
        }
        // A passage becomes "playing" when its first sample leaves the ring for
        // the device, not when the mixer starts on it -- those are ~14 s apart
        // [REQ-AUD-164]. `frames_mixed` against the ring depth is the test, and
        // it is deliberately in FRAMES rather than milliseconds of position: a
        // resumed passage starts at a non-zero position and would otherwise
        // announce itself the instant it was admitted.
        let ring = self.out_buffered_frames() as u64;
        if let Some(l) = self.live.iter().rev().find(|l| l.frames_mixed > ring) {
            self.shown = Some((l.entry.clone(), self.audible_ms(l)));
        } else if let Some((entry, _)) = self.shown.clone() {
            // Still audible though no longer mixed: keep it, and keep its
            // position moving, rather than blanking the display mid-passage.
            let pos = self
                .live
                .iter()
                .find(|l| l.entry.passage_id == entry.passage_id)
                .map(|l| self.audible_ms(l))
                // Gone from `live` but still sounding: what it had mixed, less
                // what is still queued behind it. As the ring drains this
                // advances to the end of the passage on its own
                // `[REQ-VIS-240]`.
                .or_else(|| match self.draining {
                    Some((id, at, since)) if id == entry.passage_id => {
                        // Capped at the passage's own end: the clock must not
                        // run past the music, however long it sits there.
                        let moved = at + since.elapsed().as_millis() as u64;
                        Some(moved.min(entry.duration_ms()))
                    }
                    _ => None,
                });
            if let Some(p) = pos {
                self.shown = Some((entry, p));
            }
        }
    }

    /// Write the snapshot everything else reads.
    fn publish(&mut self) {
        self.published = self.shown.as_ref().map(|(e, _)| e.passage_id);
        if let Ok(mut s) = self.state.lock() {
            s.playing = self.playing;
            s.current = self.shown.as_ref().map(|(e, _)| e.clone());
            s.position_ms = self.shown.as_ref().map(|(_, p)| *p).unwrap_or(0);
            // What is still to come FOR THE LISTENER `[REQ-AUD-164]`. A
            // passage leaves the queue when the mixer admits it, which is up to
            // a ring's depth before anyone hears it -- so the next track used
            // to vanish from "Coming up" while the current one was still
            // playing. Anything admitted but not yet audible belongs at the
            // top of the list, not gone from it.
            let shown_id = self.shown.as_ref().map(|(e, _)| e.passage_id);
            let after = match self.live.iter().position(|l| Some(l.entry.passage_id) == shown_id) {
                Some(i) => i + 1,
                // The heard passage has finished mixing, so everything still
                // in `live` is ahead of the listener.
                None => 0,
            };
            let pending = self.live.iter().skip(after).map(|l| l.entry.clone());
            s.queue_len = self.live.len().saturating_sub(after) + self.queue.len();
            s.queue = pending
                .chain(self.queue.iter().cloned())
                .take(crate::QUEUE_SHOWN)
                .collect();
            s.mixing_ahead = self.live.len().saturating_sub(after);
            s.volume = self.volume;
            s.skip_fade_ms = self.skip_fade_ms;
            s.skip_lead_ms = self.skip_lead_ms;
            s.resume_save_ms = self.resume_save_ms;
            // Every listener setting, published. These were declared on
            // `PlayerState` and read by the settings page but never filled, so
            // the page would have offered a confident **0 hours** for both
            // suppression windows — a control showing a value the engine does
            // not hold is worse than one showing nothing.
            s.skip_suppress_h = self.skip_suppress_h;
            s.dequeue_suppress_h = self.dequeue_suppress_h;
            s.queue_depth = self.queue.min_depth;
            s.sample_interval_ms = self.sample_interval_ms;
            s.cue_sheets = self.cue_sheets;
            s.covers = self.covers;
            s.lyrics_cache = self.lyrics_cache;
            s.lyrics_sidecar = self.lyrics_sidecar;
            s.active_streams = self.live.len();
            s.underrun_samples = self.underruns_playing;
            s.underruns_since_reset =
                self.underruns_playing.saturating_sub(self.underrun_baseline);
            s.underruns_since = self.underrun_since;
            s.lock_failures = self.path.ring.as_ref().map_or(0, |r| r.diagnostics().1);
            s.out_recoveries = self.path.recoveries();
            s.output_buffered =
                self.path.ring.as_ref().map(|r| r.buffered()).unwrap_or(0);
        }
    }
}

/// Save on the way out, wherever "out" happens to be. Callers exit by several
/// routes -- a Shutdown command, an empty queue, or simply dropping the engine
/// -- and putting the final save in each of them would be three chances to
/// forget one. The periodic save is up to `resume_save_ms` stale, so this is what
/// keeps a clean exit from costing those seconds.
impl Drop for Engine {
    fn drop(&mut self) {
        self.persist(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(id: i64, path: &str) -> QueueEntry {
        QueueEntry {
        qid: 0, // stamped by Queue on the way in
            passage_id: id,
            path: PathBuf::from(path),
            start_ms: 0,
            end_ms: 5_000,
            file_ms: 0,
            lead_in_ms: 0,
            lead_out_ms: 0,
            gain_db: 0.0,
            mbid: None,
            naming: Default::default(),
        }
    }

    #[test]
    fn pause_stops_submission_without_losing_the_queue() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.enqueue(entry(1, "missing.mp3"));
        h.send(Command::Pause);
        e.tick();
        assert!(!h.snapshot().playing);
        assert!(!e.is_shutdown(), "pause must not shut the engine down");
    }

    #[test]
    fn shutdown_ends_the_loop() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::Shutdown);
        e.tick();
        assert!(e.is_shutdown());
    }

    /// The two-state model: pausing must not drain or halt the producers.
    #[test]
    fn pausing_keeps_the_producers_running() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::Pause);
        for _ in 0..3 {
            e.tick();
        }
        assert!(!h.snapshot().playing);
        assert!(!e.is_shutdown(), "pause is a playback state, not a shutdown");
    }

    #[test]
    fn a_dropped_sender_stops_the_engine() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        drop(h);
        e.tick();
        assert!(e.is_shutdown(), "a vanished controller must not leave it running");
    }

    #[test]
    fn the_next_passage_is_opened_before_it_is_needed() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::Play);
        e.enqueue(entry(1, "does-not-exist.mp3"));
        e.tick();
        // Nothing openable here, but the point stands: preparing must not leave
        // an unopenable passage in place to be retried on every tick forever.
        assert_eq!(h.snapshot().queue_len, 0);
        assert!(e.ready.is_none());
        assert!(!e.is_shutdown());
    }

    /// Preparation must not make the passage sound. `live` is what the mixer
    /// sums; a prepared passage that leaked into it would play over the top of
    /// whatever is already going.
    #[test]
    fn a_prepared_passage_is_not_yet_sounding() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::Play);
        e.enqueue(entry(1, "does-not-exist.mp3"));
        e.tick();
        assert_eq!(h.snapshot().active_streams, 0, "prepared is not live");
    }

    /// A browsed passage goes to the TOP of the queue, which is the next thing
    /// heard `[REQ-VIS-180]`. It went in second for a while, on the mistaken
    /// idea that the sounding passage occupied slot zero -- it does not; it is
    /// in `live` and out of the queue entirely.
    const DQ_MBID: &str = "aaaaaaaa-0000-0000-0000-000000000007";

    /// Write a decodable WAV of `ms` milliseconds and return its path.
    ///
    /// **Generated, not committed.** The oldest gap in this engine's tests was
    /// that judging a play needs audio to play, and there were no fixtures; a
    /// binary in the repository would have been one answer, but symphonia's
    /// default features already bring a RIFF reader and a PCM codec, so silence
    /// can simply be written on the spot. Silence decodes to frames like
    /// anything else, and frames are what the clock counts.
    fn wav_of(ms: u64) -> std::path::PathBuf {
        const RATE: u32 = 44_100;
        const CH: u16 = 2;
        let frames = (RATE as u64 * ms / 1000) as u32;
        let data = frames * CH as u32 * 2;
        let mut v = Vec::with_capacity(44 + data as usize);
        v.extend(b"RIFF");
        v.extend((36 + data).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes()); // PCM
        v.extend(CH.to_le_bytes());
        v.extend(RATE.to_le_bytes());
        v.extend((RATE * CH as u32 * 2).to_le_bytes());
        v.extend((CH * 2).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend(data.to_le_bytes());
        v.resize(44 + data as usize, 0);

        let p = std::env::temp_dir().join(format!(
            "vaino_fixture_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&p, v).unwrap();
        p
    }

    /// Tick until a condition holds, or give up. The silent path is not paced
    /// by real time, so this converges in milliseconds.
    fn tick_until(e: &mut Engine, mut done: impl FnMut(&Engine) -> bool) -> bool {
        for _ in 0..200_000 {
            if done(e) {
                return true;
            }
            e.tick();
        }
        false
    }

    fn plays(st: &PlayerStore) -> i64 {
        st.play_count()
    }

    /// A passage heard past its threshold is written to history
    /// `[SPEC-PLAY-010]`.
    ///
    /// The engine's half of that rule had never been tested end to end: the
    /// judgement was covered in `scrobble`, but nothing had ever played a
    /// passage through this engine and checked that a row appeared.
    #[test]
    fn a_passage_heard_past_its_threshold_is_recorded() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(4_000);
        let mut ent = entry(41, wav.to_str().unwrap());
        ent.end_ms = 4_000;
        ent.mbid = Some("aaaaaaaa-0000-0000-0000-000000000041".into());
        e.enqueue(ent);
        h.send(Command::Play);
        e.drain_commands();

        assert!(tick_until(&mut e, |_| plays(&st) > 0), "a play should have been written");
        assert_eq!(plays(&st), 1, "and exactly one, however many ticks it took");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// The primitive the deferred correction runs on `[REQ-VIS-250]`: grows
    /// with real time from wherever the ring left off, and never claims more
    /// than the passage actually is -- the same cap `advance_shown` applies
    /// to the position display for the identical reason `[REQ-VIS-240]`.
    #[test]
    fn pending_finish_estimate_advances_with_the_clock_and_caps_at_span() {
        let p = PendingFinish { play_id: 1, span_ms: 100, at_ms: 80, since: Instant::now() };
        assert_eq!(p.estimate(), 80, "nothing has elapsed yet");
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(p.estimate(), 100, "the clock must not run past the passage's own length");
    }

    /// A skip or a seek wipes the ring outright `[REQ-VIS-250]`: whatever a
    /// still-draining play had reached by then is all it is ever going to
    /// reach, so it must be written now rather than left waiting for a tail
    /// that no longer exists.
    #[test]
    fn a_skip_resolves_a_still_draining_correction_rather_than_losing_it() {
        let (_st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        // As if an earlier passage had just departed and was still draining
        // when this test picks the story up.
        let play_id = e
            .store
            .as_ref()
            .unwrap()
            .record_play(90, Some("aaaaaaaa-0000-0000-0000-000000000090"), 400, 500)
            .unwrap();
        e.pending_finish =
            Some(PendingFinish { play_id, span_ms: 500, at_ms: 400, since: Instant::now() });

        // Something else has to be live for `skip` to act on at all.
        let wav = wav_of(2_000);
        let mut ent = entry(91, wav.to_str().unwrap());
        ent.end_ms = 2_000;
        e.enqueue(ent);
        h.send(Command::Play);
        e.drain_commands();
        assert!(tick_until(&mut e, |eng| eng.snapshot_live() > 0), "the second passage should have started");

        h.send(Command::Skip);
        e.drain_commands();

        assert!(e.pending_finish.is_none(), "the interrupted correction must be resolved, not left pending");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let heard: i64 = conn
            .query_row("SELECT heard_ms FROM listener_play_history WHERE play_id = ?1", [play_id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            (400..=500).contains(&heard),
            "the resolved figure should be at least what had already drained: got {heard}"
        );
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// The figure `record_play` writes the instant the threshold is crossed
    /// is not the last word `[REQ-VIS-250]`. A passage played all the way
    /// through must read as EXACTLY whole once its drain tail has actually
    /// finished, not frozen at whatever the ring still had queued behind it
    /// the moment the decoder ran out.
    ///
    /// **Needs a real ring, not `silent()`.** A ring of `None` reports zero
    /// frames buffered, always -- `audible_ms` never lags `played_ms` there,
    /// so the bug this guards against cannot occur in that fixture no matter
    /// what the code does. Draining it fully after every tick stands in for
    /// a device consuming what was just mixed, except on the very tick the
    /// passage exhausts: that tick's freshly-submitted tail is still sitting
    /// in the ring when `retire_finished` reads it, which is exactly the gap
    /// a real device leaves too.
    #[test]
    fn a_completed_play_is_corrected_with_what_was_actually_heard() {
        let (st, path) = store();
        let ring = crate::output::OutputRing::new(20_000, crate::output::Volume::new(1.0));
        let (mut e, h) = Engine::new(crate::path::PathHandle::with_ring(ring.clone()), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(600);
        let mut ent = entry(46, wav.to_str().unwrap());
        ent.end_ms = 600;
        ent.mbid = Some("aaaaaaaa-0000-0000-0000-000000000046".into());
        e.enqueue(ent);
        h.send(Command::Play);
        e.drain_commands();

        // Left holding a steady backlog rather than drained to empty: a real
        // device keeps a roughly constant amount of latency, not zero, and
        // draining fully every tick let the exhaustion tick's own tiny
        // remainder round down to nothing in milliseconds -- proving
        // nothing about the bug this exists to catch. ~90 ms of stereo
        // audio at 44.1 kHz.
        const KEEP: usize = 8_000;
        let drain = |ring: &crate::output::OutputRing| {
            let mut st = ring.state.lock().unwrap();
            let len = st.ring.len();
            if len > KEEP {
                let mut scratch = vec![0.0f32; len - KEEP];
                st.ring.read(&mut scratch);
            }
        };
        // Let it actually start sounding before watching for it to finish --
        // `live` is empty both before the first admission and after the last
        // retirement, and only the second one is the departure this test
        // wants.
        assert!(tick_until(&mut e, |eng| eng.snapshot_live() > 0), "the passage should have started");
        while e.snapshot_live() > 0 {
            e.tick();
            drain(&ring);
        }

        assert_eq!(plays(&st), 1, "a play should have been written");
        let pending = e.pending_finish.as_ref().expect(
            "a naturally-exhausted play must be deferred, not finalised on the spot",
        );
        assert!(
            pending.at_ms < pending.span_ms,
            "the ring should still have been holding some of the tail: at {} of {}",
            pending.at_ms, pending.span_ms
        );

        // Still sitting at whatever `record_play` wrote when the threshold
        // was first crossed -- the old, buggy answer -- until the clock
        // says the tail is done.
        let row = |c: &rusqlite::Connection| -> (i64, i64) {
            c.query_row("SELECT heard_ms, span_ms FROM listener_play_history", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
        };
        let conn = rusqlite::Connection::open(&path).unwrap();
        let (before, span_check) = row(&conn);
        assert_eq!(span_check, 600);
        assert!(before < 600, "not corrected yet: read {before} of 600 before the clock catches up");

        // The clock closes the rest of the gap, not the ring -- which by now
        // holds nothing at all for this passage.
        std::thread::sleep(std::time::Duration::from_millis(150));
        e.tick();
        assert!(e.pending_finish.is_none(), "the drain estimate should have resolved by now");

        let (heard, span) = row(&conn);
        assert_eq!(span, 600);
        assert_eq!(heard, span, "played to the end must read as exactly whole");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// A passage dropped before its threshold is **not** a play, and is
    /// suppressed instead `[SPEC-PLAY-050]`.
    #[test]
    fn a_passage_cut_short_becomes_a_skip_not_a_play() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let long = wav_of(30_000);
        let short = wav_of(1_000);
        let mut a = entry(42, long.to_str().unwrap());
        a.end_ms = 30_000; // threshold 15 s, and we will not get near it
        a.mbid = Some("aaaaaaaa-0000-0000-0000-000000000042".into());
        let mut b = entry(43, short.to_str().unwrap());
        b.end_ms = 1_000;
        e.enqueue(a);
        e.enqueue(b);
        h.send(Command::Play);
        e.drain_commands();

        // Let it sound briefly, then move on -- far short of fifteen seconds.
        for _ in 0..40 {
            e.tick();
        }
        h.send(Command::Skip);
        e.drain_commands();
        assert!(
            tick_until(&mut e, |_| {
                st.last_rejected(crate::db::Rejection::Skip).map(|m| !m.is_empty()).unwrap_or(false)
            }),
            "the abandoned passage should have been suppressed"
        );
        assert_eq!(plays(&st), 0, "and it must never have become a play");
        let _ = std::fs::remove_file(&long);
        let _ = std::fs::remove_file(&short);
        let _ = std::fs::remove_file(&path);
    }

    /// The **last** passage, abandoned with nothing behind it, must still be
    /// judged `[SPEC-PLAY-050]`.
    ///
    /// The suspicious case: `record_play` reads the head to do its work, so a
    /// queue that empties leaves it nothing to read. If the rejection is only
    /// written when some *other* passage takes the head, then skipping the last
    /// track of an evening suppresses nothing and the Director may offer it
    /// straight back.
    #[test]
    fn skipping_the_last_passage_still_suppresses_it() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(30_000);
        let mut only = entry(44, wav.to_str().unwrap());
        only.end_ms = 30_000;
        only.mbid = Some("aaaaaaaa-0000-0000-0000-000000000044".into());
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..40 {
            e.tick();
        }

        h.send(Command::Skip);
        e.drain_commands();
        let judged = tick_until(&mut e, |_| {
            st.last_rejected(crate::db::Rejection::Skip).map(|m| !m.is_empty()).unwrap_or(false)
        });
        assert!(judged, "an abandoned last passage must still be suppressed");
        assert_eq!(plays(&st), 0);
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// **A handoff is not a rejection** `[SPEC-BK-065]`.
    ///
    /// The fade is the same one a skip uses, so without the distinction the
    /// passage the listener is *still hearing on the other backend* would earn
    /// a 156-hour suppression for the crime of changing rooms.
    #[test]
    fn handing_a_passage_over_does_not_suppress_it() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(30_000);
        let mut only = entry(51, wav.to_str().unwrap());
        only.end_ms = 30_000;
        only.mbid = Some("aaaaaaaa-0000-0000-0000-000000000051".into());
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..40 {
            e.tick();
        }
        assert!(e.head_position().is_some(), "something is playing to hand over");

        e.hand_off_to_silence(600);
        for _ in 0..40 {
            e.tick();
        }

        assert!(
            st.last_rejected(crate::db::Rejection::Skip).unwrap().is_empty(),
            "it moved backends; it was not declined"
        );
        assert_eq!(plays(&st), 0, "and it did not earn a play either");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// **Switching to Sonos mid-crossfade disturbs nothing already sounding**
    /// `[Sonos/SONOS010 §8]` -- the same guarantee `ReopenOutput` and a
    /// Bluetooth reconnect already give, for the same reason: the handler
    /// touches only `self.path` and `self.sonos_ring`, never `self.live`, so
    /// there is nothing here that *could* restart, skip, or reorder a fade
    /// already in flight.
    #[cfg(feature = "sonos")]
    #[test]
    fn switching_to_sonos_mid_crossfade_leaves_the_fade_alone() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);

        // Built to overlap: the first passage's lead-out and the second's
        // lead-in both span nearly the whole first passage, so admission
        // fires almost immediately rather than waiting out a full track.
        let wav_a = wav_of(1_000);
        let mut a = entry(61, wav_a.to_str().unwrap());
        a.end_ms = 1_000;
        a.lead_out_ms = 900;
        let wav_b = wav_of(3_000);
        let mut b = entry(62, wav_b.to_str().unwrap());
        b.end_ms = 3_000;
        b.lead_in_ms = 900;
        e.enqueue(a);
        e.enqueue(b);
        h.send(Command::Play);
        e.drain_commands();

        assert!(
            tick_until(&mut e, |eng| eng.snapshot_live() == 2),
            "both sides of the fade should be live at once"
        );
        let before: Vec<i64> = e.live.iter().map(|l| l.entry.passage_id).collect();
        let head_before = e.head_position();

        let ring = crate::output::OutputRing::new(2_000, crate::output::Volume::new(1.0));
        h.send(Command::SetSonosRing(Some(ring)));
        e.drain_commands();
        assert_eq!(
            e.live.iter().map(|l| l.entry.passage_id).collect::<Vec<_>>(),
            before,
            "choosing Sonos must not touch which passages are live"
        );
        assert!(e.playing, "the session is still playing; only the local device went quiet");

        e.tick();
        let head_mid = e.head_position();
        assert_eq!(
            head_mid.map(|(id, _)| id),
            head_before.map(|(id, _)| id),
            "the head did not jump to a different passage"
        );

        h.send(Command::SetSonosRing(None));
        e.drain_commands();
        assert_eq!(
            e.live.iter().map(|l| l.entry.passage_id).collect::<Vec<_>>(),
            before,
            "and returning to local output must not touch it either"
        );

        let _ = std::fs::remove_file(&wav_a);
        let _ = std::fs::remove_file(&wav_b);
    }

    /// **The clock keeps running after the mixer has finished** `[REQ-VIS-240]`.
    ///
    /// A passage leaves `live` when its decoder is exhausted, a ring's depth
    /// before its last sample is heard. The display used to stop there — about
    /// fifteen seconds short of the end of every track.
    #[test]
    fn a_finished_passage_keeps_its_position_moving_while_it_is_heard() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        let wav = wav_of(1_500);
        let mut only = entry(91, wav.to_str().unwrap());
        only.end_ms = 1_500;
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..400 {
            e.tick();
        }

        // Mixed to the end and retired, but the listener is still hearing it.
        assert_eq!(e.snapshot_live(), 0, "the mixer has finished with it");
        let (id, _, _) = e.draining.expect("it is remembered as still sounding");
        assert_eq!(id, 91);

        let first = e.shown.as_ref().map(|(_, p)| *p).unwrap_or(0);
        std::thread::sleep(std::time::Duration::from_millis(120));
        e.tick();
        let later = e.shown.as_ref().map(|(_, p)| *p).unwrap_or(0);

        assert!(later >= first, "the position must not go backwards");
        assert!(later <= 1_500, "and must not run past the music: {later}");
        let _ = std::fs::remove_file(&wav);
    }
    /// **Restarting the count moves a mark, it does not clear the counter**
    /// `[REQ-VIS-230]`.
    ///
    /// The cumulative figure answers "has this ever glitched" and must survive
    /// somebody restarting the display, which answers "is it glitching now".
    #[test]
    fn restarting_the_underrun_count_keeps_the_cumulative_one() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.underruns_playing = 4_096;
        let before = e.underrun_since;

        h.send(Command::RestartUnderruns);
        e.drain_commands();

        assert_eq!(e.underruns_playing, 4_096, "the counter itself is untouched");
        assert_eq!(e.underrun_baseline, 4_096, "the mark moved to where it was");
        assert!(e.underrun_since >= before, "and the moment was taken");

        // More arrive after the restart, and only those are shown.
        e.underruns_playing += 500;
        assert_eq!(e.underruns_playing - e.underrun_baseline, 500);
    }

    /// A player that has never been asked still says when it started counting,
    /// rather than leaving the label empty `[REQ-VIS-230]`.
    #[test]
    fn the_count_knows_when_it_started_without_being_asked() {
        let (e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        assert!(e.underrun_since > 0, "seeded at construction, not at first reset");
        assert_eq!(e.underrun_baseline, 0);
    }
    /// **Seeking to the end must not earn a play** `[SPEC-PLAY-012]`.
    ///
    /// The whole reason the engine now measures heard time rather than reading
    /// the position: a jump to the last chorus puts the position past any
    /// threshold instantly, and nobody listened to the distance.
    #[test]
    fn seeking_past_the_threshold_does_not_earn_a_play() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(60_000);
        let mut only = entry(81, wav.to_str().unwrap());
        only.end_ms = 60_000;
        only.mbid = Some("aaaaaaaa-0000-0000-0000-000000000081".into());
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..20 {
            e.tick();
        }

        // Straight past the half-way mark, which is the threshold for a minute.
        e.seek_to(55_000);
        for _ in 0..40 {
            e.tick();
        }

        assert_eq!(plays(&st), 0, "the distance was travelled, not heard");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// And the seek itself lands where it was asked to, alone.
    #[test]
    fn a_seek_moves_the_passage_and_leaves_one_thing_sounding() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        let wav = wav_of(60_000);
        let mut only = entry(82, wav.to_str().unwrap());
        only.end_ms = 60_000;
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..20 {
            e.tick();
        }

        e.seek_to(30_000);

        assert_eq!(e.snapshot_live(), 1, "it lands alone");
        let (id, at) = e.head_position().expect("still playing");
        assert_eq!(id, 82, "the same passage, moved");
        assert!(at >= 30_000, "at the point asked for, not back at the start: {at}");
        let _ = std::fs::remove_file(&wav);
    }

    /// A seek past the end would open a decoder with nothing to decode.
    #[test]
    fn a_seek_beyond_the_span_is_clamped_inside_it() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        let wav = wav_of(10_000);
        let mut only = entry(83, wav.to_str().unwrap());
        only.end_ms = 10_000;
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..20 {
            e.tick();
        }

        e.seek_to(999_999);

        assert_eq!(e.snapshot_live(), 1, "still sounding rather than ended");
        let _ = std::fs::remove_file(&wav);
    }

    /// Seeking with nothing playing is a no-op, not a panic.
    #[test]
    fn seeking_with_nothing_playing_does_nothing() {
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.seek_to(5_000);
        assert_eq!(e.snapshot_live(), 0);
    }

    /// **A passage that arrives already counted is not counted again**
    /// `[SPEC-BK-065]`.
    ///
    /// `[SPEC-BK-037]` named this hazard before there was code to have it: a
    /// passage crossing mid-play can be judged by both sides. It earns neither a
    /// second play nor — the worse failure — a rejection for a passage that
    /// played.
    #[test]
    fn a_passage_adopted_mid_play_is_not_counted_twice() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(2_000);
        let mut only = entry(61, wav.to_str().unwrap());
        only.end_ms = 2_000;
        only.mbid = Some("aaaaaaaa-0000-0000-0000-000000000061".into());
        e.enqueue(only);
        // The other backend already wrote this one's play.
        e.adopt_counted(61);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..400 {
            e.tick();
        }

        assert_eq!(plays(&st), 0, "its play is already in the history, written by the other side");
        assert!(
            st.last_rejected(crate::db::Rejection::Skip).unwrap().is_empty(),
            "and it must not be suppressed either -- it played"
        );
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// The adoption is for **one** passage, not a standing amnesty. The next one
    /// is judged normally, or a single handoff would silence accounting for the
    /// rest of the session.
    #[test]
    fn adopting_one_passage_does_not_excuse_the_next() {
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.adopt_counted(70);
        assert!(!e.head_counted(), "nothing is playing yet");
        // A different passage arriving must not consume the adoption.
        e.enqueue(entry(71, "b.mp3"));
        assert!(!e.head_counted());
    }

    /// The same fade, asked for the other way, still suppresses — or the
    /// distinction above would have quietly disabled skip suppression.
    #[test]
    fn an_ordinary_fade_to_silence_still_suppresses() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let wav = wav_of(30_000);
        let mut only = entry(52, wav.to_str().unwrap());
        only.end_ms = 30_000;
        only.mbid = Some("aaaaaaaa-0000-0000-0000-000000000052".into());
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..40 {
            e.tick();
        }

        e.fade_to_silence(600);
        let judged = tick_until(&mut e, |_| {
            st.last_rejected(crate::db::Rejection::Skip).map(|m| !m.is_empty()).unwrap_or(false)
        });

        assert!(judged, "a fade that is not a handoff is still the listener leaving");
        let _ = std::fs::remove_file(&wav);
        let _ = std::fs::remove_file(&path);
    }

    /// The position that crosses is the **audible** one `[SPEC-BK-065]`.
    ///
    /// `played_ms` runs ahead by whatever is sitting in the output ring — about
    /// 14 s here — and handing that over would start the other side that far
    /// into the future.
    #[test]
    fn the_position_handed_over_is_the_one_being_heard() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        let wav = wav_of(30_000);
        let mut only = entry(53, wav.to_str().unwrap());
        only.end_ms = 30_000;
        e.enqueue(only);
        h.send(Command::Play);
        e.drain_commands();
        for _ in 0..40 {
            e.tick();
        }

        let (id, pos) = e.head_position().expect("something is playing");
        assert_eq!(id, 53);
        assert!(pos < 30_000, "inside the span it is playing, not past it");
        let _ = std::fs::remove_file(&wav);
    }

    /// A fade to silence empties the engine, so a handoff leaves nothing
    /// behind to play `[SPEC-BK-030]`.
    ///
    /// The queue in particular: those passages are being rebuilt on the other
    /// backend, and one left here would be promoted into the fade and start
    /// playing on a side the listener has just left.
    #[test]
    fn a_fade_to_silence_leaves_nothing_queued_or_sounding() {
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.enqueue(entry(1, "a.mp3"));
        e.enqueue(entry(2, "b.mp3"));
        // Deliberately NOT ticked: a tick would try to open these names, fail,
        // and empty the queue for the wrong reason. What is under test is that
        // the fade takes the queue, not that a missing file does.
        assert_eq!(e.queued().count(), 2, "something to lose");

        e.fade_to_silence(600);

        assert!(e.queued().next().is_none(), "the queue went with the handoff");
        assert_eq!(e.snapshot_live(), 0, "and nothing is sounding");
    }

    /// With no output there is nothing to fade, and it says so rather than
    /// claiming a smooth stop that never happened `[PI3-API-030]`.
    #[test]
    fn a_silent_path_reports_a_cut_not_a_fade() {
        use crate::switch::{FadeOut, Stopped};
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.enqueue(entry(1, "a.mp3"));
        assert_eq!(e.fade_out(600), Stopped::Cut, "no ring, so no fade to claim");
    }

    /// The listener's own skip shape is not disturbed by a handoff borrowing it.
    #[test]
    fn fading_out_restores_the_skip_setting() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::SetSkipFade(1_500));
        e.drain_commands();
        e.fade_to_silence(200);
        std::thread::sleep(Engine::PUBLISH_EVERY + std::time::Duration::from_millis(20));
        e.tick();
        assert_eq!(h.snapshot().skip_fade_ms, 1_500, "borrowed, then given back");
    }

    /// Every listener setting must reach the snapshot the settings page reads.
    ///
    /// Two of them did not. `skip_suppress_h` and `dequeue_suppress_h` were
    /// declared on `PlayerState`, serialised by the web layer and read by the
    /// skin, but never assigned in `publish` — so the page would have shown a
    /// confident **0 hours** for both while the engine held 156 and 18. A
    /// control displaying a value the engine does not hold is worse than one
    /// displaying nothing, because it invites a person to trust it.
    #[test]
    fn every_listener_setting_reaches_the_snapshot() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.tick();
        let s = h.snapshot();
        assert_eq!(s.skip_suppress_h, crate::SKIP_SUPPRESS_H);
        assert_eq!(s.dequeue_suppress_h, crate::DEQUEUE_SUPPRESS_H);
        assert_eq!(s.queue_depth, 3, "the depth the engine was built with");
        assert_eq!(s.sample_interval_ms, crate::SAMPLE_INTERVAL_MS);

        // And a change reaches it too, rather than only the default.
        h.send(Command::SetSkipSuppress(72));
        h.send(Command::SetDequeueSuppress(9));
        h.send(Command::SetQueueDepth(8));
        h.send(Command::SetSampleInterval(2_500));
        e.drain_commands();
        // Publishing is throttled to `PUBLISH_EVERY`, so a change made just
        // after one publish is not visible until the next. Waiting past the
        // window is what a browser polling the snapshot does anyway.
        std::thread::sleep(Engine::PUBLISH_EVERY + std::time::Duration::from_millis(20));
        e.tick();
        let s = h.snapshot();
        assert_eq!(
            (s.skip_suppress_h, s.dequeue_suppress_h, s.queue_depth, s.sample_interval_ms),
            (72, 9, 8, 2_500)
        );
    }

    /// Out-of-range values are clamped rather than accepted, on the way in from
    /// a browser as much as from a file.
    #[test]
    fn settings_from_outside_are_clamped() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::SetQueueDepth(0));
        h.send(Command::SetSampleInterval(1));
        e.drain_commands();
        std::thread::sleep(Engine::PUBLISH_EVERY + std::time::Duration::from_millis(20));
        e.tick();
        let s = h.snapshot();
        assert_eq!(s.queue_depth, crate::QUEUE_DEPTH_MIN, "a depth of zero has no lookahead");
        assert_eq!(s.sample_interval_ms, crate::SAMPLE_INTERVAL_MIN_MS);
    }

    /// Taking a passage out of the queue before it plays is a rejection, and
    /// the shorter kind `[SPEC-PLAY-055]`.
    #[test]
    fn removing_a_queued_passage_records_a_dequeue() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let mut ent = entry(7, "a.mp3");
        ent.mbid = Some(DQ_MBID.into());
        e.enqueue(ent);
        let qid = e.queued().next().expect("queued").qid;

        h.send(Command::RemoveQueued(qid));
        e.drain_commands();

        assert!(e.queued().next().is_none(), "it really left the queue");
        let deq = st.last_rejected(crate::db::Rejection::Dequeue).unwrap();
        assert_eq!(deq.len(), 1, "one dequeue recorded");
        assert!(deq.contains_key(DQ_MBID), "recorded against its recording");
        assert!(
            st.last_rejected(crate::db::Rejection::Skip).unwrap().is_empty(),
            "a removal is not a skip: they earn different windows"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Removing a passage that is not there records nothing. Without the
    /// existence check a stale id from a browser would suppress a recording the
    /// listener never touched.
    #[test]
    fn removing_a_passage_that_is_not_queued_records_nothing() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let mut ent = entry(7, "a.mp3");
        ent.mbid = Some(DQ_MBID.into());
        e.enqueue(ent);

        h.send(Command::RemoveQueued(4242)); // never a real qid
        e.drain_commands();

        assert!(e.queued().next().is_some(), "the real entry is untouched");
        assert!(
            st.last_rejected(crate::db::Rejection::Dequeue).unwrap().is_empty(),
            "nothing was removed, so nothing was declined"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A passage the engine could not open leaves the queue too, and it must
    /// leave **no mark** `[REQ-PD-112]`. A failure is not a preference, and
    /// suppressing a recording because its file was missing would punish the
    /// listener for a fault they did not commit.
    #[test]
    fn a_passage_that_would_not_open_is_not_a_rejection() {
        let (st, path) = store();
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.attach_store(PlayerStore::open(&path).unwrap());

        let mut ent = entry(9, "no-such-file-anywhere.mp3");
        ent.mbid = Some(DQ_MBID.into());
        e.enqueue(ent);
        for _ in 0..8 {
            e.tick();
        }

        assert!(!e.take_dropped().is_empty(), "the unopenable passage was dropped");
        assert!(
            st.last_rejected(crate::db::Rejection::Dequeue).unwrap().is_empty()
                && st.last_rejected(crate::db::Rejection::Skip).unwrap().is_empty(),
            "a file that would not open must suppress nothing"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn enqueue_next_puts_a_passage_first_in_the_queue() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.enqueue(entry(1, "a.mp3"));
        e.enqueue(entry(2, "b.mp3"));
        h.send(Command::EnqueueNext(entry(99, "browsed.mp3")));
        e.drain_commands();
        let ids: Vec<i64> = e.queued().map(|q| q.passage_id).collect();
        assert_eq!(ids, vec![99, 1, 2], "browsed passage is next up");
    }

    /// The same for a batch, in the order it was given.
    #[test]
    fn a_batch_queued_next_goes_to_the_top_in_order() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        e.enqueue(entry(1, "a.mp3"));
        h.send(Command::EnqueueMany(
            vec![entry(10, "x.mp3"), entry(11, "y.mp3")],
            Placement::Next,
        ));
        e.drain_commands();
        let ids: Vec<i64> = e.queued().map(|q| q.passage_id).collect();
        assert_eq!(ids, vec![10, 11, 1]);
    }

    #[test]
    fn an_unopenable_passage_is_skipped_not_fatal() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 3);
        h.send(Command::Play);
        e.enqueue(entry(1, "does-not-exist.mp3"));
        e.tick();
        assert_eq!(h.snapshot().queue_len, 0, "bad passage must leave the queue");
        assert_eq!(h.snapshot().active_streams, 0, "and must not become live");
        assert!(!e.is_shutdown(), "and must not end playback");
    }

    #[test]
    fn shortfall_reports_the_replenishment_need() {
        let (mut e, _h) = Engine::new(crate::path::PathHandle::silent(), 3);
        assert_eq!(e.shortfall(), 3);
        e.enqueue(entry(1, "a.mp3"));
        assert_eq!(e.shortfall(), 2);
    }

    #[test]
    fn idle_requires_the_output_to_have_drained() {
        let mut s = PlayerState::default();
        assert!(s.is_idle(), "empty everything is idle");
        s.output_buffered = 4096;
        assert!(!s.is_idle(), "buffered audio means playback is still in progress");
    }

    fn store() -> (PlayerStore, PathBuf) {
        let p = std::env::temp_dir().join(format!(
            "vaino_eng_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&p);
        (PlayerStore::open(&p).unwrap(), p)
    }

    /// Without a store the engine must behave identically, not panic or stall.
    #[test]
    fn persistence_is_optional() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        h.send(Command::Play);
        e.tick();
        assert!(h.snapshot().playing);
    }

    #[test]
    fn play_state_reaches_the_database_on_change() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        e.attach_store(PlayerStore::open(&path).unwrap());

        h.send(Command::Play);
        e.tick();
        assert_eq!(st.load().unwrap(), Some((None, 0, true)), "play must be saved at once");

        h.send(Command::Pause);
        e.tick();
        assert_eq!(
            st.load().unwrap(),
            Some((None, 0, false)),
            "a state change must not wait for the throttle"
        );
        let _ = std::fs::remove_file(path);
    }

    /// Resuming must report position within the PASSAGE. If it restarted at
    /// zero, the next save would move backwards and resume would walk to the
    /// start of the track a restart at a time.
    #[test]
    fn resume_reports_position_within_the_passage() {
        let f = crate::decoder::tests::tmp("resume");
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        let mut ent = entry(1, f.to_str().unwrap());
        ent.end_ms = 5_000;
        e.resume_at(2_000);
        e.enqueue(ent);
        h.send(Command::Play);
        e.tick();
        let pos = h.snapshot().position_ms;
        assert!(
            (2_000..2_400).contains(&pos),
            "resumed passage reported {pos} ms, expected ~2000"
        );
        let _ = std::fs::remove_file(f);
    }

    /// One-shot: the passage after the resumed one starts where the library
    /// says, not two seconds in.
    #[test]
    fn the_resume_offset_applies_only_once() {
        let f = crate::decoder::tests::tmp("once");
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        let mut a = entry(1, f.to_str().unwrap());
        a.end_ms = 1_000;
        let mut b = entry(2, f.to_str().unwrap());
        b.end_ms = 5_000;
        e.resume_at(2_000);
        e.enqueue(a);
        e.enqueue(b);
        h.send(Command::Play);
        for _ in 0..400 {
            e.tick();
            if h.snapshot().current.as_ref().map(|c| c.passage_id) == Some(2) {
                break;
            }
        }
        let s = h.snapshot();
        assert_eq!(s.current.map(|c| c.passage_id), Some(2), "second passage never started");
        assert!(s.position_ms < 1_000, "second passage inherited the resume offset");
        let _ = std::fs::remove_file(f);
    }

    /// A resume point past the end (the passage was re-trimmed since) replays
    /// the passage rather than decoding nothing.
    #[test]
    fn an_out_of_range_resume_point_does_not_strand_the_passage() {
        let f = crate::decoder::tests::tmp("clamp");
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        let mut ent = entry(1, f.to_str().unwrap());
        ent.end_ms = 3_000;
        e.resume_at(99_000);
        e.enqueue(ent);
        h.send(Command::Play);
        e.tick();
        assert_eq!(h.snapshot().active_streams, 1, "clamped resume must still open");
        let _ = std::fs::remove_file(f);
    }

    /// Exiting by dropping the engine -- the common path when the queue runs
    /// dry -- must still leave a current resume point.
    #[test]
    fn the_final_position_is_saved_on_drop() {
        let (st, path) = store();
        {
            let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
            e.attach_store(PlayerStore::open(&path).unwrap());
            h.send(Command::Play);
            e.tick();
        }
        assert_eq!(st.load().unwrap().map(|s| s.2), Some(true), "drop must flush the state");
        let _ = std::fs::remove_file(path);
    }

    /// Silence while paused is the design, not a fault. If it counted, the
    /// metric would be dominated by idle time and could never flag a real one.
    #[test]
    fn underruns_while_paused_are_not_counted() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 1);
        h.send(Command::Pause);
        for _ in 0..5 {
            e.tick();
        }
        assert_eq!(h.snapshot().underrun_samples, 0);
    }

    #[test]
    fn state_is_published_every_tick() {
        let (mut e, h) = Engine::new(crate::path::PathHandle::silent(), 2);
        e.enqueue(entry(7, "x.mp3"));
        e.tick();
        let s = h.snapshot();
        assert_eq!(s.queue_len + s.active_streams, 0, "unopenable passage clears");
    }
}
