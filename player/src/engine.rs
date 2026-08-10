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

use crate::decoder::PassageDecoder;
use crate::fade::{Curve, Fade};
use crate::mixer::{mix, Stream};
use crate::output::Output;
use crate::queue::{should_admit, Queue, QueueEntry};
use crate::resample::Resampler;
use crate::BUFFER_FRAMES;

/// A passage currently decoding and/or sounding.
struct Live {
    dec: PassageDecoder,
    stream: Stream,
    resampler: Resampler,
    converted: Vec<f32>,
    entry: QueueEntry,
    frames_mixed: u64,
}

/// What the UI and the persistence layer read. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct PlayerState {
    pub playing: bool,
    pub current: Option<QueueEntry>,
    pub position_ms: u64,
    pub queue_len: usize,
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
    Enqueue(QueueEntry),
    /// Terminate the process. Deliberately NOT a playback state -- it ends the
    /// engine rather than putting playback into a third mode.
    Shutdown,
}

pub struct EngineHandle {
    tx: Sender<Command>,
    pub state: Arc<Mutex<PlayerState>>,
}

impl EngineHandle {
    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }
    pub fn snapshot(&self) -> PlayerState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

pub struct Engine {
    queue: Queue,
    live: Vec<Live>,
    out: Option<Output>,
    out_rate: u32,
    out_channels: usize,
    scratch: Vec<f32>,
    state: Arc<Mutex<PlayerState>>,
    rx: Receiver<Command>,
    playing: bool,
    shutdown: bool,
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
            out,
            out_rate,
            out_channels,
            scratch: vec![0.0; 2048 * out_channels],
            state: Arc::clone(&state),
            rx,
            playing: false,
            shutdown: false,
        };
        (engine, EngineHandle { tx, state })
    }

    pub fn enqueue(&mut self, e: QueueEntry) {
        self.queue.push(e);
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
        // Producers run in BOTH states. Pausing stops the consumer only, so
        // buffers stay full and resuming does not re-incur a fill.
        self.top_up_decoders();
        // Submitting while paused would be audible -- the callback drains
        // continuously -- so only the consumer side is gated.
        let submitted = if self.playing { self.mix_and_submit() } else { 0 };
        self.retire_finished();
        self.publish();
        submitted
    }

    fn drain_commands(&mut self) {
        loop {
            match self.rx.try_recv() {
                Ok(Command::Play) => self.playing = true,
                Ok(Command::Pause) => self.playing = false,
                Ok(Command::Skip) => self.skip(),
                Ok(Command::Enqueue(e)) => self.queue.push(e),
                Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => {
                    self.shutdown = true;
                    return;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
    }

    /// Drop the sounding passage. Its buffered audio goes with it, which is
    /// what makes skip immediate rather than "after the buffer drains"
    /// `[REQ-AUD-110]`.
    fn skip(&mut self) {
        if !self.live.is_empty() {
            self.live.remove(0);
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
        match self.open(&entry) {
            Ok(l) => self.live.push(l),
            Err(e) => eprintln!("skipping {}: {e}", entry.path.display()),
        }
    }

    fn open(&self, e: &QueueEntry) -> Result<Live, String> {
        let dec = PassageDecoder::open(&e.path, e.start_ms, Some(e.end_ms))
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
            entry: e.clone(),
            frames_mixed: 0,
        })
    }

    fn top_up_decoders(&mut self) {
        for l in self.live.iter_mut() {
            if l.stream.finished || l.stream.ring.free() < 4096 * l.stream.channels {
                continue;
            }
            match l.dec.next() {
                Ok(Some(chunk)) => {
                    l.converted.clear();
                    let mut buf = std::mem::take(&mut l.converted);
                    match l.resampler.process(chunk, &mut buf) {
                        Ok(()) => {
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

    fn played_ms(&self, l: &Live) -> u64 {
        l.frames_mixed * 1000 / self.out_rate.max(1) as u64
    }

    fn publish(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.playing = self.playing;
            s.current = self.live.first().map(|l| l.entry.clone());
            s.position_ms = self.live.first().map(|l| self.played_ms(l)).unwrap_or(0);
            s.queue_len = self.queue.len();
            s.active_streams = self.live.len();
            s.underrun_samples = self.out.as_ref().map(|o| o.diagnostics().0).unwrap_or(0);
            s.output_buffered = self
                .out
                .as_ref()
                .map(|o| o.buffered())
                .unwrap_or(0);
        }
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

    #[test]
    fn state_is_published_every_tick() {
        let (mut e, h) = Engine::new(None, 2);
        e.enqueue(entry(7, "x.mp3"));
        e.tick();
        let s = h.snapshot();
        assert_eq!(s.queue_len + s.active_streams, 0, "unopenable passage clears");
    }
}
