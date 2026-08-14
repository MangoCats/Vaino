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
use crate::output::Output;
use crate::queue::{should_admit, Queue, QueueEntry};
use crate::resample::Resampler;
use crate::BUFFER_FRAMES;

/// How often the resume point reaches the database during steady playback.
/// Losing at most this much position to a power cut is a fair trade for not
/// writing to storage thousands of times a second.
const SAVE_EVERY: Duration = Duration::from_secs(5);

/// A passage currently decoding and/or sounding.
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
    /// Master volume, 0.0 to 1.0.
    pub volume: f32,
    /// Skip transition shape `[REQ-AUD-162]`, so a UI can show what it will do.
    pub skip_fade_ms: u64,
    pub skip_lead_ms: u64,
    pub active_streams: usize,
    pub underrun_samples: u64,
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
    Enqueue(QueueEntry),
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
    out: Option<Output>,
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
    saved: Option<(i64, bool)>,
    /// The last passage written to play history, so a passage is recorded
    /// once however many ticks it sounds for.
    recorded: Option<i64>,
    /// Underruns that happened while PLAYING. Under the two-state model the
    /// device callback drains continuously, so a paused player underruns
    /// forever -- counting those would bury the fault this number exists to
    /// expose [REQ-AUD-142].
    underruns_playing: u64,
    last_raw_underruns: u64,
}

impl Engine {
    /// `out` of `None` runs the full pipeline into a discard sink — useful for
    /// tests and headless hosts, but note it reports no device rate and so
    /// cannot catch a resampling fault `[REQ-HW-147]`.
    pub fn new(out: Option<Output>, min_depth: usize) -> (Self, EngineHandle) {
        let (tx, rx) = channel();
        let state = Arc::new(Mutex::new(PlayerState::default()));
        let out_rate = out.as_ref().map(|o| o.sample_rate).unwrap_or(44_100);
        let out_channels = out.as_ref().map(|o| o.channels).unwrap_or(2);
        let engine = Self {
            queue: Queue::new(min_depth),
            live: Vec::new(),
            ready: None,
            out,
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
            saved: None,
            recorded: None,
            underruns_playing: 0,
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
    /// Returns samples submitted, so a caller can pace itself when there is no
    /// device applying back-pressure.
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
        let submitted = if self.playing { self.mix_and_submit() } else { 0 };
        self.retire_finished();
        self.record_play();
        self.publish();
        self.persist(false);
        submitted
    }

    fn drain_commands(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(Command::Play) => self.set_playing(true),
                Ok(Command::Pause) => self.set_playing(false),
                Ok(Command::Skip) => self.skip(),
                Ok(Command::SetSkipFade(ms)) => {
                    self.skip_fade_ms = ms.min(crate::SKIP_FADE_MAX_MS);
                }
                Ok(Command::SetSkipLead(ms)) => {
                    self.skip_lead_ms =
                        ms.clamp(crate::SKIP_LEAD_MIN_MS, crate::SKIP_LEAD_MAX_MS);
                }
                Ok(Command::SetVolume(v)) => {
                    self.volume = v.clamp(0.0, 1.0);
                    // Straight to the device: the callback applies it, so the
                    // change is heard now rather than a ring-depth later.
                    if let Some(o) = &self.out {
                        o.volume.set(self.volume);
                    }
                }
                Ok(Command::Enqueue(e)) => self.queue.push(e),
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
        if let Some(o) = &self.out {
            o.set_playing(on);
        }
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

        // How much of the outgoing survives the cut sets how much of the
        // incoming overlaps it. Asked before the cut, since afterwards the
        // answer is by definition the fade length.
        let have = self.out.as_ref().map_or(0, |o| o.buffered()).min(fade_samples);
        let mut overlay = vec![0.0f32; have.saturating_sub(lead_samples)];
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

        if let Some(o) = &self.out {
            o.begin_skip_transition(
                self.skip_fade_ms,
                self.skip_lead_ms,
                Curve::Exponential,
                &overlay,
            );
        }
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
            Err(e) => eprintln!("skipping {}: {e}", entry.path.display()),
        }
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
            if l.stream.finished || l.stream.ring.free() < 4096 * l.stream.channels {
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
        let room = match &self.out {
            Some(o) => o.free(),
            None => self.scratch.len(),
        };
        // Whole frames only; a partial frame would offset every later sample.
        let want = room.min(self.scratch.len()) / self.out_channels * self.out_channels;
        if want == 0 {
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
        match &self.out {
            Some(o) => {
                let taken = o.submit(&self.scratch[..filled]);
                debug_assert_eq!(taken, filled, "output accepted less than it reported free");
                taken
            }
            None => filled, // discard sink: report accepted so callers advance
        }
    }

    fn retire_finished(&mut self) {
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
        self.out.as_ref().map(|o| o.buffered() / self.out_channels.max(1)).unwrap_or(0)
    }

    /// Write the resume point. Throttled, because a tick is sub-millisecond and
    /// an SQLite write per tick would dominate the loop; `force` bypasses it for
    /// shutdown. Writes immediately when the passage or play state changes, so
    /// the interesting transitions are never the ones lost to a power cut.
    fn persist(&mut self, force: bool) {
        let Some(store) = &self.store else { return };
        let key = (self.live.first().map(|l| l.entry.passage_id).unwrap_or(-1), self.playing);
        let changed = self.saved != Some(key);
        if !force && !changed && self.last_save.elapsed() < SAVE_EVERY {
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

    /// Write a play to history the moment a passage begins sounding
    /// `[REQ-PD-110]`.
    ///
    /// At the START of playback, not on completion. Rotation exists to space
    /// out what the listener has *encountered*, and a track skipped after ten
    /// seconds has been encountered — suppressing it for a while is the wanted
    /// behaviour, not a bug. It also matches MuLibPlay, whose own note says the
    /// history structures update "as each new track finishes playing (or is put
    /// in the play queue)".
    ///
    /// A failure here must never interrupt playback: history is what the next
    /// selection reads, not what this one depends on.
    fn record_play(&mut self) {
        if !self.playing {
            return;
        }
        let Some(live) = self.live.first() else { return };
        let id = live.entry.passage_id;
        if self.recorded == Some(id) {
            return;
        }
        self.recorded = Some(id);
        if let Some(store) = &self.store {
            if let Err(e) = store.record_play(id, live.entry.mbid.as_deref()) {
                eprintln!("record play: {e}");
            }
        }
    }

    fn publish(&mut self) {
        // Attribute the increment before publishing: silence during a pause is
        // expected, silence during playback is the bug worth reporting.
        let raw = self.out.as_ref().map(|o| o.diagnostics().0).unwrap_or(0);
        let delta = raw.saturating_sub(self.last_raw_underruns);
        self.last_raw_underruns = raw;
        if self.playing {
            self.underruns_playing += delta;
        }
        if let Ok(mut s) = self.state.lock() {
            s.playing = self.playing;
            s.current = self.live.first().map(|l| l.entry.clone());
            s.position_ms = self.live.first().map(|l| self.audible_ms(l)).unwrap_or(0);
            s.queue_len = self.queue.len();
            s.queue = self.queue.iter().take(12).cloned().collect();
            s.volume = self.volume;
            s.skip_fade_ms = self.skip_fade_ms;
            s.skip_lead_ms = self.skip_lead_ms;
            s.active_streams = self.live.len();
            s.underrun_samples = self.underruns_playing;
            s.output_buffered = self
                .out
                .as_ref()
                .map(|o| o.buffered())
                .unwrap_or(0);
        }
    }
}

/// Save on the way out, wherever "out" happens to be. Callers exit by several
/// routes -- a Shutdown command, an empty queue, or simply dropping the engine
/// -- and putting the final save in each of them would be three chances to
/// forget one. The periodic save is up to `SAVE_EVERY` stale, so this is what
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
            passage_id: id,
            path: PathBuf::from(path),
            start_ms: 0,
            end_ms: 5_000,
            lead_in_ms: 0,
            lead_out_ms: 0,
            gain_db: 0.0,
            mbid: None,
        }
    }

    #[test]
    fn pause_stops_submission_without_losing_the_queue() {
        let (mut e, h) = Engine::new(None, 3);
        e.enqueue(entry(1, "missing.mp3"));
        h.send(Command::Pause);
        e.tick();
        assert!(!h.snapshot().playing);
        assert!(!e.is_shutdown(), "pause must not shut the engine down");
    }

    #[test]
    fn shutdown_ends_the_loop() {
        let (mut e, h) = Engine::new(None, 3);
        h.send(Command::Shutdown);
        e.tick();
        assert!(e.is_shutdown());
    }

    /// The two-state model: pausing must not drain or halt the producers.
    #[test]
    fn pausing_keeps_the_producers_running() {
        let (mut e, h) = Engine::new(None, 3);
        h.send(Command::Pause);
        for _ in 0..3 {
            e.tick();
        }
        assert!(!h.snapshot().playing);
        assert!(!e.is_shutdown(), "pause is a playback state, not a shutdown");
    }

    #[test]
    fn a_dropped_sender_stops_the_engine() {
        let (mut e, h) = Engine::new(None, 3);
        drop(h);
        e.tick();
        assert!(e.is_shutdown(), "a vanished controller must not leave it running");
    }

    #[test]
    fn the_next_passage_is_opened_before_it_is_needed() {
        let (mut e, h) = Engine::new(None, 3);
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
        let (mut e, h) = Engine::new(None, 3);
        h.send(Command::Play);
        e.enqueue(entry(1, "does-not-exist.mp3"));
        e.tick();
        assert_eq!(h.snapshot().active_streams, 0, "prepared is not live");
    }

    #[test]
    fn an_unopenable_passage_is_skipped_not_fatal() {
        let (mut e, h) = Engine::new(None, 3);
        h.send(Command::Play);
        e.enqueue(entry(1, "does-not-exist.mp3"));
        e.tick();
        assert_eq!(h.snapshot().queue_len, 0, "bad passage must leave the queue");
        assert_eq!(h.snapshot().active_streams, 0, "and must not become live");
        assert!(!e.is_shutdown(), "and must not end playback");
    }

    #[test]
    fn shortfall_reports_the_replenishment_need() {
        let (mut e, _h) = Engine::new(None, 3);
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
        let (mut e, h) = Engine::new(None, 1);
        h.send(Command::Play);
        e.tick();
        assert!(h.snapshot().playing);
    }

    #[test]
    fn play_state_reaches_the_database_on_change() {
        let (st, path) = store();
        let (mut e, h) = Engine::new(None, 1);
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
        let (mut e, h) = Engine::new(None, 1);
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
        let (mut e, h) = Engine::new(None, 1);
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
        let (mut e, h) = Engine::new(None, 1);
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
            let (mut e, h) = Engine::new(None, 1);
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
        let (mut e, h) = Engine::new(None, 1);
        h.send(Command::Pause);
        for _ in 0..5 {
            e.tick();
        }
        assert_eq!(h.snapshot().underrun_samples, 0);
    }

    #[test]
    fn state_is_published_every_tick() {
        let (mut e, h) = Engine::new(None, 2);
        e.enqueue(entry(7, "x.mp3"));
        e.tick();
        let s = h.snapshot();
        assert_eq!(s.queue_len + s.active_streams, 0, "unopenable passage clears");
    }
}
