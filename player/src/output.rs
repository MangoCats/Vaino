//! Audio output: a cpal stream whose callback drains a ring buffer.
//!
//! The device is local to the process `[REQ-AUD-150]`: remote browsers control
//! the player, they never receive audio.
//!
//! The callback runs on a real-time thread. It must not allocate, must not
//! block, and must always return promptly -- so it does exactly one thing:
//! copy from a ring buffer, and count the shortfall when there isn't enough.
//! All decoding, fading and mixing happen on ordinary threads and reach the
//! callback only through [`OutputState::ring`].
//!
//! This is McRhythm's split `[DBD-PARAM-030]`: a mixer thread fills an output
//! ring, the callback drains it. The ring decouples the two so a slow decode
//! costs latency rather than a dropout.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::fade::{Curve, Fade};
use crate::mixer::RingBuffer;

/// Shared between the mixer thread and the audio callback.
///
/// Only the ring. The counters used to live here and moved out deliberately --
/// see [`Counts`].
pub struct OutputState {
    pub ring: RingBuffer,
}

impl OutputState {
    fn new(capacity: usize) -> Self {
        Self { ring: RingBuffer::new(capacity) }
    }
}

/// What the callback observed, counted OUTSIDE the ring's mutex.
///
/// These were fields of [`OutputState`], which put them behind the very lock
/// whose contention one of them exists to measure. `lock_failures` was
/// therefore incremented under a *second* `try_lock` that could fail for the
/// same reason the first did: the figure was a lower bound, and biased low
/// exactly when contention was worst -- the case anyone reading it most wants
/// to see. A counter that under-reports the harder the fault gets is worse
/// than no counter, because it reads as reassurance `[GDE-FBD-100]`.
///
/// Atomics also take two lock acquisitions per tick off the mixer, which is
/// the larger practical win: the engine read both of these every tick to
/// publish them.
#[derive(Clone, Default)]
pub struct Counts {
    /// Samples the callback wanted but could not get. Non-zero means the
    /// producer is not keeping up -- the diagnostic that matters most here,
    /// and the one a silent fallback would otherwise hide `[REQ-VIS-140]`.
    underruns: Arc<std::sync::atomic::AtomicU64>,
    /// Times the callback could not take the lock at all. Distinguished from a
    /// genuine underrun because the fix is different: contention argues for a
    /// lock-free ring, starvation argues for more buffering.
    lock_failures: Arc<std::sync::atomic::AtomicU64>,
    /// The last sample emitted, as bits, so a miss can ramp down from the
    /// waveform rather than stepping off it.
    last: Arc<std::sync::atomic::AtomicU32>,
    /// Set by a miss, cleared by the fill that follows it, which ramps back in.
    resuming: Arc<AtomicBool>,
}

impl Counts {
    pub fn underruns(&self) -> u64 { self.underruns.load(Ordering::Relaxed) }
    pub fn lock_failures(&self) -> u64 { self.lock_failures.load(Ordering::Relaxed) }
    /// The last sample emitted, for tests and for reasoning about a miss.
    pub fn last_sample(&self) -> f32 { f32::from_bits(self.last.load(Ordering::Relaxed)) }
}

/// Master volume, as `f32` bits in an atomic.
///
/// Deliberately OUTSIDE the mutex that guards the ring. The audio callback must
/// never block, and it must be able to change level even on the tick where it
/// cannot take that lock. An `AtomicU32` load is lock-free; a `Mutex<f32>`
/// would put a second lock on the real-time path for one number.
#[derive(Clone)]
pub struct Volume(Arc<std::sync::atomic::AtomicU32>);

/// The bottom of the fader, in dB relative to full scale `[REQ-AUD-154]`.
///
/// The control's whole travel is this range, linear in dB: -72.0 at the bottom,
/// 0.0 at the top. Wider than MuLibPlay's 64 dB, which is the one number in
/// this design not inherited from it.
pub const FADER_MIN_DB: f32 = -72.0;

impl Volume {
    /// Level in dB relative to full scale, to amplitude.
    ///
    /// The control is graduated in decibels rather than in a percentage of
    /// travel, so equal movement is an equal change in loudness -- the ear
    /// hears ratios, and a fader linear in *amplitude* spends its top half on
    /// differences barely distinguishable from full. It also means the caption
    /// is the control's own value, with no second copy of the curve in the
    /// browser to be wrong about it.
    pub fn amplitude_at_db(db: f32) -> f32 {
        10f32.powf(db.clamp(FADER_MIN_DB, 0.0) / 20.0)
    }

    /// The inverse, for putting the knob back where the listener left it: the
    /// resume point stores amplitude, but the control is graduated in dB.
    ///
    /// Silence has no logarithm, so it reads as the bottom of the travel.
    pub fn db_for(amplitude: f32) -> f32 {
        if amplitude <= 0.0 {
            return FADER_MIN_DB;
        }
        (20.0 * amplitude.log10()).clamp(FADER_MIN_DB, 0.0)
    }

    pub fn new(v: f32) -> Self {
        Self(Arc::new(std::sync::atomic::AtomicU32::new(v.to_bits())))
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(std::sync::atomic::Ordering::Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.clamp(0.0, 1.0).to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// Everything about the output that can cross a thread boundary
/// `[SPEC-APS-070]`.
///
/// `cpal::Stream` is `!Send`, so the device itself must stay on whichever
/// thread opened it. Everything the mixer actually needs -- the ring, the
/// volume, the flags -- is shareable, and lives here.
///
/// The split is what lets the supervisor own the device's lifecycle on its own
/// thread while the engine keeps writing audio, and it is what removes the
/// engine's ability to open, close or wait for a device at all. Three separate
/// times, blocking work reached the mixer through that capability
/// `[GDE-FBD-090]`.
///
/// `failed` and `silent` are deliberately **stable across reattachment**. An
/// earlier version replaced the `failed` Arc on every `recover`, which is
/// harmless while one object owns it and silently wrong the moment anyone else
/// holds a clone: they would watch a flag nothing sets any more.
#[derive(Clone)]
pub struct OutputRing {
    pub state: Arc<Mutex<OutputState>>,
    /// Applied in the callback `[REQ-AUD-152]`, so a change is heard at the
    /// device rather than after the ring drains.
    pub volume: Volume,
    /// Set by the stream's error callback, cleared by a successful attach.
    failed: Arc<AtomicBool>,
    /// Feed zeros rather than stop, so the link survives a pause.
    silent: Arc<AtomicBool>,
    /// The device's own rate and channel count, which a reattach may change --
    /// so they are read rather than remembered.
    rate: Arc<std::sync::atomic::AtomicU32>,
    chans: Arc<std::sync::atomic::AtomicU32>,
    /// Counted without the lock, so the callback can record the tick on which
    /// it could not take it.
    pub counts: Counts,
}

impl OutputRing {
    // `pub(crate)`, not private: `path.rs` builds one to attach a real
    // device, and the engine's own tests build one to simulate a device that
    // is buffering audio without a real one on the machine `[REQ-VIS-250]`.
    pub(crate) fn new(capacity: usize, volume: Volume) -> Self {
        Self {
            state: Arc::new(Mutex::new(OutputState::new(capacity))),
            volume,
            failed: Arc::new(AtomicBool::new(false)),
            silent: Arc::new(AtomicBool::new(false)),
            rate: Arc::new(std::sync::atomic::AtomicU32::new(44_100)),
            chans: Arc::new(std::sync::atomic::AtomicU32::new(2)),
            counts: Counts::default(),
        }
    }

    /// The device's rate and channel count as they are *now*. Read rather than
    /// cached: a reattach onto a different sink can change both.
    pub fn sample_rate(&self) -> u32 { self.rate.load(Ordering::Relaxed) }
    pub fn channels(&self) -> usize { self.chans.load(Ordering::Relaxed) as usize }

    /// Mark the output as needing recovery.
    pub fn mark_failed(&self) { self.failed.store(true, Ordering::Relaxed); }

    /// Has the stream reported an error it will not recover from itself?
    pub fn failed(&self) -> bool { self.failed.load(Ordering::Relaxed) }

    /// Silence without stopping the device `[PI3-OPEN-020]`.
    pub fn set_silent(&self, on: bool) { self.silent.store(on, Ordering::Relaxed); }

    /// Space available in the output ring, in samples.
    pub fn free(&self) -> usize {
        self.state.lock().map(|s| s.ring.free()).unwrap_or(0)
    }

    /// Hand mixed audio to the output.
    ///
    /// Returns `(accepted, free_after)`. The second value exists so a caller
    /// does not need a separate `free()` on the next pass: between now and
    /// then the callback only ever *drains*, so this is a lower bound on the
    /// room that will be available, and writing that much always fits.
    pub fn submit(&self, samples: &[f32]) -> (usize, usize) {
        self.state
            .lock()
            .map(|mut s| {
                let took = s.ring.write(samples);
                (took, s.ring.free())
            })
            .unwrap_or((0, 0))
    }

    /// Samples submitted but not yet consumed by the device.
    /// Cut the backlog short, fade it out, and lay the next passage over its
    /// tail `[REQ-AUD-162]`.
    ///
    /// Skip could not be prompt for exactly the reason pause could not be
    /// `[REQ-AUD-142]`: the ring holds ~14 s of mixed audio and the callback
    /// drains it whatever the mixer does. Dropping the passage upstream stops
    /// only the *adding* to a backlog the listener must still sit through --
    /// measured at 14.0 s from button to new music.
    ///
    /// So the ring is cut to the length of the fade, the fade is applied to
    /// what remains, and `overlay` -- the incoming passage, already decoded and
    /// with its own fade-in already applied -- is summed in starting `lead_ms`
    /// along. The two therefore overlap for `fade_ms - lead_ms`.
    ///
    /// **The fade is applied here, not in the callback**, precisely because the
    /// incoming audio lands in these same samples: a fade-out running in the
    /// callback would drag the newcomer down with the passage it is replacing.
    ///
    /// All of it happens under one lock, so no callback can observe a ring that
    /// is cut but not yet faded. Returns `(faded, overlaid)` in samples.
    pub fn begin_skip_transition(
        &self,
        fade_ms: u64,
        lead_ms: u64,
        curve: Curve,
        overlay: &[f32],
    ) -> (usize, usize) {
        let ch = self.channels().max(1);
        let rate = self.sample_rate() as u64;
        let fade_samples = (fade_ms * rate / 1000) as usize * ch;
        let lead_samples = (lead_ms * rate / 1000) as usize * ch;
        // A blocking lock is safe here: this runs on the mixer thread, and the
        // callback only ever tries.
        let Ok(mut s) = self.state.lock() else { return (0, 0) };

        let kept = s.ring.truncate(fade_samples);
        // Span the fade over what is actually there. Holding it to the
        // requested length when the ring is shallower -- at startup, say --
        // would cut off part-way down the curve, at an audible step.
        let frames = (kept / ch) as u64;
        // The envelope is looked up, not computed, because this runs with the
        // output lock held and the callback only ever TRIES for that lock. A
        // 10 s fade is ~441k frames; two `powf` each would be tens of
        // milliseconds under the lock, and every callback that fails to take it
        // emits silence -- a click at the exact moment of a skip. A table of
        // `FADE_TABLE` entries is built once, off the lock, and indexed here.
        let table = fade_table(curve);
        {
            let (front, back) = s.ring.as_mut_slices();
            let mut frame = 0u64;
            for run in [front, back] {
                for f in run.chunks_mut(ch) {
                    let g = table[gain_index(frame, frames)];
                    f.iter_mut().for_each(|x| *x *= g);
                    frame += 1;
                }
            }
        }
        let placed = s.ring.mix_at(lead_samples, overlay);
        (kept, placed)
    }

    pub fn buffered(&self) -> usize {
        self.state.lock().map(|s| s.ring.len()).unwrap_or(0)
    }

    /// `(underrun_samples, lock_failures)`. Two atomic loads -- no lock, so
    /// reading the diagnostics cannot itself cause the contention it reports.
    pub fn diagnostics(&self) -> (u64, u64) {
        (self.counts.underruns(), self.counts.lock_failures())
    }
}

pub struct Output {
    // Also the pause control: the callback drains whether or not we are
    // submitting, so stopping the CONSUMER means stopping this stream.
    //
    // Optional only so that recovery can drop it and leave the device closed
    // for the moment between attempts.
    stream: Option<cpal::Stream>,
    /// The shareable half. Cloned to whoever mixes; never replaced.
    pub ring: OutputRing,
    pub device_name: String,
    /// The device *selector* rather than the device: recovery has to re-resolve
    /// the name, because the sink it names may be a different ALSA object by
    /// the time it comes back.
    requested: Option<String>,
}

#[derive(Debug)]
pub enum OutputError {
    NoDevice,
    Config(String),
    Build(String),
}

impl std::fmt::Display for OutputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputError::NoDevice => write!(f, "no default output device"),
            OutputError::Config(e) => write!(f, "output config: {e}"),
            OutputError::Build(e) => write!(f, "build stream: {e}"),
        }
    }
}

/// Resolution of the precomputed fade envelope `[REQ-AUD-162]`.
///
/// 1024 steps over any fade length: at the 10 s maximum that is one entry per
/// ~10 ms, and the curve moves by well under the ~1 dB anyone can hear across
/// a step. The point is to keep `powf` off the output lock entirely.
const FADE_TABLE: usize = 1024;

/// How long the audio callback will wait for the ring before giving way.
///
/// A fraction of a percent of the callback's own budget, against producer holds
/// measured in microseconds.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_micros(300);

/// Samples over which a miss is faded in or out. ~1.5 ms at 44.1 kHz stereo.
const DECLICK_SAMPLES: usize = 128;

fn fade_table(curve: Curve) -> Vec<f32> {
    let fade = Fade { curve, frames: FADE_TABLE as u64, fade_in: false };
    (0..FADE_TABLE).map(|i| fade.gain_at(i as u64)).collect()
}

fn gain_index(frame: u64, frames: u64) -> usize {
    if frames == 0 {
        return 0;
    }
    ((frame * FADE_TABLE as u64) / frames).min(FADE_TABLE as u64 - 1) as usize
}

impl Output {
    /// Open the default output device.
    pub fn open(ring_capacity_samples: usize) -> Result<Self, OutputError> {
        Self::open_device(None, ring_capacity_samples)
    }

    /// Open a named output device, or the default when `name` is `None`.
    ///
    /// Named selection exists because the output channel is a deployment
    /// choice, not a fixed property: an appliance may be built around a
    /// Bluetooth sink, an I2S DAC HAT, a USB DAC or HDMI, and those differ in
    /// boot behaviour as well as in device name `[IMPL-AUD-010]`. Matching is a
    /// case-insensitive substring so a configuration file can say "bluealsa"
    /// or "hifiberry" without encoding an exact ALSA string.
    ///
    /// `ring_capacity_samples` is the decoupling buffer between the mixer
    /// thread and the callback.
    pub fn open_device(name: Option<&str>, ring_capacity_samples: usize)
        -> Result<Self, OutputError>
    {
        let ring = OutputRing::new(ring_capacity_samples, Volume::new(1.0));
        let requested = name.map(str::to_string);
        let (stream, device_name) = Self::attach(requested.as_deref(), &ring)?;
        Ok(Self { stream: Some(stream), ring, device_name, requested })
    }

    /// The shareable half, for whoever mixes into it `[SPEC-APS-070]`.
    pub fn ring(&self) -> OutputRing {
        self.ring.clone()
    }

    /// Open the device and start a stream against an *existing* ring and
    /// volume.
    ///
    /// Split out of `open_device` so recovery can replace a dead stream
    /// without replacing the buffer the mixer writes into. Swapping the state
    /// too would leave the mixer filling a ring that nothing reads, which is a
    /// worse failure than the one being recovered from `[IMPL-AUD-020]`.
    fn attach(name: Option<&str>, ring: &OutputRing)
        -> Result<(cpal::Stream, String), OutputError>
    {
        let state = Arc::clone(&ring.state);
        let volume = ring.volume.clone();
        let host = cpal::default_host();
        let device = match name {
            Some(want) => {
                let want = want.to_lowercase();
                host.output_devices()
                    .map_err(|e| OutputError::Config(e.to_string()))?
                    .find(|d| {
                        d.name().map(|n| n.to_lowercase().contains(&want)).unwrap_or(false)
                    })
                    .ok_or(OutputError::NoDevice)?
            }
            None => host.default_output_device().ok_or(OutputError::NoDevice)?,
        };
        let device_name = device.name().unwrap_or_else(|_| "<unnamed>".into());
        let supported = device
            .default_output_config()
            .map_err(|e| OutputError::Config(e.to_string()))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate.0;

        let cb_state = Arc::clone(&state);

        // One closure per sample format. `fill` holds all the logic so the
        // format arms stay trivial and cannot diverge.
        //
        // Recording the failure is the point. A Bluetooth sink that goes away
        // reports EIO here exactly once and then simply stops calling back, so
        // a handler that only logs leaves a player which is silent, holds no
        // link, and still looks healthy from every side `[IMPL-AUD-020]`.
        // The flags belong to the RING, not to this stream, so a clone held by
        // the mixer keeps working across every reattachment.
        let failed = Arc::clone(&ring.failed);
        let silent = Arc::clone(&ring.silent);
        let cb_failed = Arc::clone(&failed);
        let err_fn = move |e| {
            eprintln!("output stream error: {e}");
            cb_failed.store(true, Ordering::Relaxed);
        };
        let stream = match sample_format {
            SampleFormat::F32 => {
                let cb_vol = volume.clone();
                let cb_silent = Arc::clone(&silent);
                let cb_counts = ring.counts.clone();
                device.build_output_stream(
                &config,
                move |out: &mut [f32], _| fill(&cb_state, &cb_vol, out, &cb_silent, &cb_counts),
                err_fn,
                None,
            )}
            SampleFormat::I16 => {
                let mut scratch: Vec<f32> = Vec::new();
                let cb_vol = volume.clone();
                let cb_silent = Arc::clone(&silent);
                let cb_counts = ring.counts.clone();
                device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &cb_vol, &mut scratch, &cb_silent, &cb_counts);
                        for (o, s) in out.iter_mut().zip(scratch.iter()) {
                            *o = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        }
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let mut scratch: Vec<f32> = Vec::new();
                let cb_vol = volume.clone();
                let cb_silent = Arc::clone(&silent);
                let cb_counts = ring.counts.clone();
                device.build_output_stream(
                    &config,
                    move |out: &mut [u16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &cb_vol, &mut scratch, &cb_silent, &cb_counts);
                        for (o, s) in out.iter_mut().zip(scratch.iter()) {
                            let v = (s.clamp(-1.0, 1.0) + 1.0) * 0.5;
                            *o = (v * u16::MAX as f32) as u16;
                        }
                    },
                    err_fn,
                    None,
                )
            }
            other => return Err(OutputError::Config(format!("unsupported format {other:?}"))),
        }
        .map_err(|e| OutputError::Build(e.to_string()))?;

        stream.play().map_err(|e| OutputError::Build(e.to_string()))?;
        // Publish what this device actually is, and clear the failure only now
        // that one has genuinely opened `[GDE-FBD-100]`.
        ring.rate.store(sample_rate, Ordering::Relaxed);
        ring.chans.store(channels as u32, Ordering::Relaxed);
        ring.failed.store(false, Ordering::Relaxed);
        Ok((stream, device_name))
    }

    /// Mark the output as needing recovery.
    ///
    /// For a reopen that failed: the device is not usable, and the retry loop
    /// is the right owner of trying again `[PI3-API-010]`.
    pub fn mark_failed(&self) {
        self.ring.mark_failed();
    }

    /// Has the stream reported an error it will not recover from itself?
    pub fn failed(&self) -> bool {
        self.ring.failed()
    }

    /// Release the device without opening another.
    ///
    /// Half of a two-step reopen `[PI3-OPEN-010]`. A stream rebuilt in the same
    /// breath as the old one is dropped loses a Bluetooth speaker about
    /// twenty-two seconds later, every time, where one opened fresh holds
    /// indefinitely; PipeWire needs a moment to finish tearing the old one
    /// down. The waiting is the CALLER's to do, on its own schedule -- sleeping
    /// here would mean sleeping on the mixer thread, which starves the ring and
    /// trades a dropout for a stutter.
    pub fn release(&mut self) {
        self.stream = None;
        self.ring.mark_failed();
    }

    /// Is the device currently released, waiting to be reopened?
    pub fn released(&self) -> bool {
        self.stream.is_none()
    }

    /// Rebuild the stream after a failure, keeping the ring and the volume.
    ///
    /// The ring is deliberately drained first. Its contents are audio that was
    /// mixed for a moment now several seconds past; playing it on reconnection
    /// would replay that moment, and a listener hears a stutter rather than a
    /// gap `[REQ-AUD-142]`. A gap is the honest rendering of a sink that went
    /// away.
    ///
    /// Returns the new device name on success. Failure is expected and not
    /// exceptional -- the sink is often still absent -- so the caller is meant
    /// to retry rather than to treat this as fatal.
    pub fn recover(&mut self) -> Result<String, OutputError> {
        // Release the dead device *before* opening it again. ALSA will refuse
        // the second open while the first handle is alive, so building the new
        // stream first and assigning over the old one -- the obvious ordering --
        // fails every time on exactly the sink this exists for.
        let (stream, device_name) = Self::attach(self.requested.as_deref(), &self.ring)?;
        // Discard what the ring holds only now that a device has actually been
        // opened. Clearing on every ATTEMPT was a bug: a retry loop against an
        // absent sink emptied the buffer twice a second, the mixer refilled it
        // each time, and the position raced ahead of a silent player
        // `[PI3-API-030]`.
        if let Ok(mut s) = self.ring.state.lock() {
            s.ring.clear();
        }
        self.stream = Some(stream);
        self.device_name = device_name.clone();
        Ok(device_name)
    }

    /// Start or stop the device callback.
    ///
    /// Pausing playback by merely not submitting is not enough: the ring holds
    /// roughly fourteen seconds, and the callback keeps draining it, so the
    /// music would play on for that long after the button was pressed. Stopping
    /// the stream leaves the ring full, which is what makes resuming instant
    /// `[REQ-AUD-142]`.
    ///
    /// Returns false if the backend refuses -- not all of them can pause -- so
    /// the caller can fall back rather than assume silence.
    pub fn set_playing(&self, on: bool) -> bool {
        // play() and pause() return different error types, so normalise early
        // rather than let that detail leak into the caller.
        let Some(stream) = self.stream.as_ref() else { return false };
        // Silence rather than a stopped device `[PI3-OPEN-020]`. The stream is
        // still started on resume in case a backend stopped it for its own
        // reasons, but pausing no longer stops it.
        self.ring.set_silent(!on);
        let r = if on {
            stream.play().map_err(|e| e.to_string())
        } else {
            Ok(())
        };
        if let Err(e) = &r {
            eprintln!("output {}: {e}", if on { "play" } else { "pause" });
        }
        r.is_ok()
    }

    /// Names of available output devices, for diagnosing a failed match.
    pub fn list_devices() -> Vec<String> {
        cpal::default_host()
            .output_devices()
            .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

}

/// The whole of the real-time path.
///
/// `try_lock` rather than `lock`: blocking here would stall the audio device.
/// On contention we wait briefly, and only then give way -- see [`LOCK_WAIT`].
///
/// The miss is counted either way, because a glitch that is recorded can be
/// fixed and a glitch that is hidden cannot.
fn fill(state: &Arc<Mutex<OutputState>>, volume: &Volume, out: &mut [f32],
        silent: &Arc<AtomicBool>, counts: &Counts) {
    // Paused means silence, NOT a stopped stream `[PI3-OPEN-020]`. A2DP tears
    // down when nothing feeds it, so stopping the device on pause loses the
    // speaker after a few minutes and makes resuming wait on a reconnect.
    // McRhythm fed silence for the same reason. The ring is left untouched, so
    // resuming is still instant `[REQ-AUD-142]`, and the shortfall is not
    // counted: this silence is intended, and inflating the underrun figure
    // would spoil the one diagnostic that matters most.
    if silent.load(Ordering::Relaxed) {
        out.iter_mut().for_each(|v| *v = 0.0);
        return;
    }
    match acquire(state) {
        Some(mut s) => {
            let got = s.ring.read(out);
            // Volume is applied HERE, at the device, not before submission.
            // The ring holds ~14 s, so scaling on the way in means a change is
            // inaudible until that much already-scaled audio has drained --
            // measured as a volume knob that appeared to lag by ten seconds or
            // more [REQ-AUD-152]. The same buffer-depth trap as pausing by
            // declining to submit [REQ-AUD-142].
            let v = volume.get();
            if v != 1.0 {
                out[..got].iter_mut().for_each(|x| *x *= v);
            }
            if got < out.len() {
                out[got..].iter_mut().for_each(|v| *v = 0.0);
                counts.underruns.fetch_add((out.len() - got) as u64, Ordering::Relaxed);
            }
            // Remember where the waveform got to, so a miss on the next buffer
            // can leave from there rather than from a step.
            counts.last.store(out.last().copied().unwrap_or(0.0).to_bits(),
                              Ordering::Relaxed);
            // Ease back in after a miss. The ring was not consumed during the
            // gap, so the signal resumes exactly where it left off -- but it
            // resumes from SILENCE, and that edge clicks just as the leaving
            // one does.
            if counts.resuming.swap(false, Ordering::Relaxed) {
                declick(out, true, 0.0);
            }
        }
        None => {
            // Ramp down from the last sample rather than dropping to zero.
            //
            // A hard-edged hole has a discontinuity at BOTH ends, which is a
            // click rather than a gap -- and clicks carry. These were reported
            // as clearly audible across a room at roughly five an hour, after
            // this code assumed 0.008% of callbacks meant nobody would notice.
            // The percentage was the wrong measure; the right one is how often
            // a listener hears something wrong.
            let from = f32::from_bits(counts.last.load(Ordering::Relaxed));
            out.iter_mut().for_each(|v| *v = 0.0);
            declick(out, false, from);
            counts.last.store(0f32.to_bits(), Ordering::Relaxed);
            counts.resuming.store(true, Ordering::Relaxed);
            counts.lock_failures.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Wait a bounded moment for the ring, then give way.
///
/// The producer's holds are microseconds -- a memcpy of at most a few thousand
/// samples -- so almost every miss is a collision with a hold that is already
/// nearly over. Giving up on the first refusal turned those into audible
/// glitches for want of a wait far shorter than the callback's own budget.
///
/// Bounded, and that is the whole safety argument: this runs on the audio
/// thread, so the wait is a small fraction of the ~23 ms available. Even if the
/// producer stalls completely, the callback still returns in time -- it simply
/// returns the fallback, exactly as before.
fn acquire(state: &Arc<Mutex<OutputState>>) -> Option<std::sync::MutexGuard<'_, OutputState>> {
    let deadline = std::time::Instant::now() + LOCK_WAIT;
    loop {
        if let Ok(g) = state.try_lock() {
            return Some(g);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::hint::spin_loop();
    }
}

/// Ramp the first `DECLICK_FRAMES` samples in or out, to kill the edge.
///
/// Short enough that the attenuation is inaudible as a level change and long
/// enough that the step becomes a slope: at 44.1 kHz this is ~1.5 ms.
fn declick(out: &mut [f32], fade_in: bool, from: f32) {
    let n = DECLICK_SAMPLES.min(out.len());
    if n == 0 {
        return;
    }
    for (i, v) in out[..n].iter_mut().enumerate() {
        let t = i as f32 / n as f32;
        if fade_in {
            *v *= t;
        } else {
            // Leaving: slide from where the waveform was to silence.
            *v = from * (1.0 - t);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Not silenced -- the ordinary case for every fill test here.
    fn audible() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }

    use super::*;

    // The real-time path is testable without a device: `fill` is a plain
    // function over shared state, which is why it was written that way.
    #[test]
    fn silence_leaves_the_ring_alone_and_counts_no_underrun() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[0.5; 8]);
        let mut out = [9.9f32; 8];
        let c = Counts::default();
        fill(&st, &Volume::new(1.0), &mut out, &Arc::new(AtomicBool::new(true)), &c);
        assert_eq!(out, [0.0; 8], "a paused stream feeds zeros");
        let s = st.lock().unwrap();
        // Both matter: the ring is what makes resuming instant, and counting
        // this as an underrun would spoil the diagnostic `[PI3-OPEN-020]`.
        assert_eq!(s.ring.len(), 8, "buffered audio is kept for the resume");
        assert_eq!(c.underruns(), 0, "intended silence is not a shortfall");
    }

    #[test]
    fn fill_pads_with_silence_and_counts_the_shortfall() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[0.5; 4]);
        let mut out = [9.9f32; 8];
        let c = Counts::default();
        fill(&st, &Volume::new(1.0), &mut out, &audible(), &c);
        assert_eq!(&out[..4], &[0.5; 4]);
        assert!(out[4..].iter().all(|v| *v == 0.0), "shortfall must be silence");
        assert_eq!(c.underruns(), 4);
    }

    /// Volume is applied at the DEVICE, not before submission. The ring holds
    /// ~14 s, so scaling on the way in makes a change inaudible until that much
    /// already-scaled audio has drained [REQ-AUD-152].
    #[test]
    fn fill_applies_volume_at_the_callback() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[1.0; 4]);
        let mut out = [0.0f32; 4];
        fill(&st, &Volume::new(0.25), &mut out, &audible(), &Counts::default());
        assert!(out.iter().all(|v| (*v - 0.25).abs() < 1e-6), "got {out:?}");
    }

    /// A volume change reaches audio already sitting in the ring -- which is
    /// the whole point: those samples have been submitted but not yet heard.
    #[test]
    fn volume_affects_audio_already_buffered() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[1.0; 8]);
        let vol = Volume::new(1.0);
        let mut a = [0.0f32; 4];
        fill(&st, &vol, &mut a, &audible(), &Counts::default());
        assert!((a[0] - 1.0).abs() < 1e-6);
        vol.set(0.5); // turned down while the rest is still queued
        let mut b = [0.0f32; 4];
        fill(&st, &vol, &mut b, &audible(), &Counts::default());
        assert!((b[0] - 0.5).abs() < 1e-6, "the change must reach buffered audio");
    }

    /// The ends must be exact: full scale must not be 0.999.
    #[test]
    fn fader_ends_are_exact() {
        assert_eq!(Volume::amplitude_at_db(0.0), 1.0);
        assert_eq!(Volume::db_for(1.0), 0.0);
        assert_eq!(Volume::db_for(Volume::amplitude_at_db(FADER_MIN_DB)), FADER_MIN_DB);
    }

    /// Equal movement, equal change in loudness -- the property the graduation
    /// exists for. The same step must be the same ratio wherever it is taken,
    /// which is what a linear fader fails to do.
    ///
    /// A halving is 6.0206 dB, not 6: rounding it to 6 leaves 0.12 % of error,
    /// enough to fail this assertion and worth stating exactly rather than
    /// loosening the tolerance to hide it.
    #[test]
    fn the_same_step_is_the_same_ratio_everywhere() {
        const HALVING_DB: f32 = 6.020_6;
        for db in [0.0, -12.0, -30.0, -60.0] {
            let ratio = Volume::amplitude_at_db(db - HALVING_DB) / Volume::amplitude_at_db(db);
            assert!((ratio - 0.5).abs() < 1e-4, "at {db} dB the step gave {ratio}");
        }
    }

    /// Known values, so a sign error or a factor of 10 versus 20 cannot pass.
    #[test]
    fn amplitudes_are_the_textbook_ones() {
        for (db, want) in [(0.0, 1.0), (-6.0206, 0.5), (-20.0, 0.1), (-40.0, 0.01)] {
            let got = Volume::amplitude_at_db(db);
            assert!((got - want).abs() < 1e-4, "{db} dB gave {got}, want {want}");
        }
    }

    /// The control runs the full 72 dB and no further: a level below the floor
    /// must land ON the floor rather than somewhere quieter still.
    #[test]
    fn fader_range_is_bounded() {
        assert_eq!(Volume::amplitude_at_db(-500.0), Volume::amplitude_at_db(FADER_MIN_DB));
        assert_eq!(Volume::amplitude_at_db(20.0), 1.0, "no gain above full scale");
        assert_eq!(Volume::db_for(0.0), FADER_MIN_DB, "silence reads as the floor");
        assert_eq!(Volume::db_for(4.0), 0.0);
    }

    /// The knob has to go back where the listener left it after a restart, and
    /// it travels through amplitude to get there.
    #[test]
    fn db_round_trips_through_amplitude() {
        for step in 0..=72 {
            let db = -(step as f32);
            let back = Volume::db_for(Volume::amplitude_at_db(db));
            assert!((back - db).abs() < 1e-3, "{db} dB came back as {back}");
        }
    }

    #[test]
    fn fader_is_monotonic() {
        let mut prev = -1.0;
        for step in (0..=72).rev() {
            let a = Volume::amplitude_at_db(-(step as f32));
            assert!(a > prev, "not increasing at -{step} dB");
            prev = a;
        }
    }

    #[test]
    fn volume_is_clamped() {
        let v = Volume::new(1.0);
        v.set(4.0);
        assert_eq!(v.get(), 1.0);
        v.set(-2.0);
        assert_eq!(v.get(), 0.0);
    }

    #[test]
    fn fill_reports_no_underrun_when_supplied() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[0.25; 8]);
        let mut out = [0.0f32; 8];
        let c = Counts::default();
        fill(&st, &Volume::new(1.0), &mut out, &audible(), &c);
        assert_eq!(c.underruns(), 0);
    }

    /// The fix for an audible fault: a hold that is nearly over must not cost
    /// a glitch. The producer's holds are microseconds, so the callback waits
    /// a bounded moment rather than giving up on the first refusal.
    #[test]
    fn a_brief_hold_costs_no_glitch() {
        let st = Arc::new(Mutex::new(OutputState::new(64)));
        st.lock().unwrap().ring.write(&[0.5; 16]);
        let c = Counts::default();
        // The contention has to be certain, or the test proves nothing: the
        // holder announces that it HAS the lock, and this thread does not move
        // until it has. Held for well under LOCK_WAIT, so the wait must win.
        let held = Arc::clone(&st);
        let taken = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&taken);
        let t = std::thread::spawn(move || {
            let g = held.lock().unwrap();
            flag.store(true, Ordering::SeqCst);
            // Busy-wait rather than sleep: thread::sleep rounds up to the
            // scheduler's tick -- milliseconds on Windows -- which would hold
            // the lock far past LOCK_WAIT and test the opposite of the point.
            let until = std::time::Instant::now() + std::time::Duration::from_micros(100);
            while std::time::Instant::now() < until {
                std::hint::spin_loop();
            }
            drop(g);
        });
        while !taken.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }
        let mut out = [9.9f32; 16];
        fill(&st, &Volume::new(1.0), &mut out, &audible(), &c);
        t.join().unwrap();
        assert_eq!(c.lock_failures(), 0, "a 100us hold must be waited out, not counted");
        assert!(out.iter().any(|v| *v != 0.0), "the audio must actually arrive");
    }

    /// A miss must not step off the waveform. The hole clicked at both edges,
    /// which is what made roughly five an hour audible across a room.
    #[test]
    fn a_miss_ramps_down_from_the_waveform_and_back_in() {
        let st = Arc::new(Mutex::new(OutputState::new(1024)));
        st.lock().unwrap().ring.write(&[1.0; 512]);
        let c = Counts::default();
        // A first, successful fill leaves the waveform at 1.0.
        let mut out = [0.0f32; 128];
        fill(&st, &Volume::new(1.0), &mut out, &audible(), &c);
        assert_eq!(c.last_sample(), 1.0, "the last sample must be remembered");

        // Now miss, with the lock genuinely held throughout.
        let held = st.lock().unwrap();
        let mut gap = [9.9f32; 256];
        fill(&st, &Volume::new(1.0), &mut gap, &audible(), &c);
        drop(held);
        assert_eq!(c.lock_failures(), 1);
        assert!(gap[0] > 0.9, "the gap must leave from the waveform, not from zero");
        assert!(gap[DECLICK_SAMPLES - 1].abs() < 0.05, "and reach silence");
        assert!(gap[DECLICK_SAMPLES..].iter().all(|v| *v == 0.0), "then stay silent");
        // Sanity: monotonically decreasing, i.e. a slope rather than a step.
        assert!(gap[..DECLICK_SAMPLES].windows(2).all(|w| w[0] >= w[1]));

        // The fill after a miss eases back in rather than stepping up.
        let mut back = [0.0f32; 256];
        fill(&st, &Volume::new(1.0), &mut back, &audible(), &c);
        assert!(back[0].abs() < 0.05, "the return must start from silence");
        assert!(back[DECLICK_SAMPLES - 1] > 0.9, "and reach the signal");
        assert!(back[..DECLICK_SAMPLES].windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn fill_emits_silence_rather_than_blocking_when_locked() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        let _held = st.lock().unwrap();
        let mut out = [9.9f32; 4];
        let c = Counts::default();
        fill(&st, &Volume::new(1.0), &mut out, &audible(), &c); // must return, not deadlock
        assert!(out.iter().all(|v| *v == 0.0));
        // The event is recorded even though the lock is STILL held -- which is
        // the whole reason the counters left the mutex. The previous version
        // counted under a second `try_lock` and so could not have passed this:
        // the only moments worth counting were the ones it could not count.
        assert_eq!(c.lock_failures(), 1, "a missed lock must be recorded, not lost");
    }
}
