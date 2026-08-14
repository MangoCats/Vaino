//! Audio output: a cpal stream whose callback drains a ring buffer.
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

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::fade::{Curve, Fade};
use crate::mixer::RingBuffer;

/// Shared between the mixer thread and the audio callback.
pub struct OutputState {
    pub ring: RingBuffer,
    /// Samples the callback wanted but could not get. Non-zero means the
    /// producer is not keeping up -- the diagnostic that matters most here,
    /// and the one a silent fallback would otherwise hide `[REQ-VIS-140]`.
    pub underrun_samples: u64,
    /// Times the callback could not take the lock at all. Distinguished from a
    /// genuine underrun because the fix is different: contention argues for a
    /// lock-free ring, starvation argues for more buffering.
    pub lock_failures: u64,
}

impl OutputState {
    fn new(capacity: usize) -> Self {
        Self { ring: RingBuffer::new(capacity), underrun_samples: 0, lock_failures: 0 }
    }
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

pub struct Output {
    // Also the pause control: the callback drains whether or not we are
    // submitting, so stopping the CONSUMER means stopping this stream.
    stream: cpal::Stream,
    pub state: Arc<Mutex<OutputState>>,
    /// Applied in the callback `[REQ-AUD-152]`, so a change is heard at the
    /// device rather than after the ring drains.
    pub volume: Volume,
    pub sample_rate: u32,
    pub channels: usize,
    pub device_name: String,
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

        let state = Arc::new(Mutex::new(OutputState::new(ring_capacity_samples)));
        let cb_state = Arc::clone(&state);

        // One closure per sample format. `fill` holds all the logic so the
        // format arms stay trivial and cannot diverge.
        let err_fn = |e| eprintln!("output stream error: {e}");
        let volume = Volume::new(1.0);
        let stream = match sample_format {
            SampleFormat::F32 => {
                let cb_vol = volume.clone();
                device.build_output_stream(
                &config,
                move |out: &mut [f32], _| fill(&cb_state, &cb_vol, out),
                err_fn,
                None,
            )}
            SampleFormat::I16 => {
                let mut scratch: Vec<f32> = Vec::new();
                let cb_vol = volume.clone();
                device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &cb_vol, &mut scratch);
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
                device.build_output_stream(
                    &config,
                    move |out: &mut [u16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &cb_vol, &mut scratch);
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
        Ok(Self { stream, state, volume, sample_rate, channels, device_name })
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
        let r = if on {
            self.stream.play().map_err(|e| e.to_string())
        } else {
            self.stream.pause().map_err(|e| e.to_string())
        };
        if let Err(e) = &r {
            eprintln!("output {}: {e}", if on { "play" } else { "pause" });
        }
        r.is_ok()
    }

    /// Space available in the output ring, in samples.
    pub fn free(&self) -> usize {
        self.state.lock().map(|s| s.ring.free()).unwrap_or(0)
    }

    /// Hand mixed audio to the output. Returns samples accepted.
    pub fn submit(&self, samples: &[f32]) -> usize {
        self.state.lock().map(|mut s| s.ring.write(samples)).unwrap_or(0)
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
        let ch = self.channels.max(1);
        let rate = self.sample_rate as u64;
        let fade_samples = (fade_ms * rate / 1000) as usize * ch;
        let lead_samples = (lead_ms * rate / 1000) as usize * ch;
        // A blocking lock is safe here: this runs on the mixer thread, and the
        // callback only ever tries.
        let Ok(mut s) = self.state.lock() else { return (0, 0) };

        let kept = s.ring.truncate(fade_samples);
        // Span the fade over what is actually there. Holding it to the
        // requested length when the ring is shallower -- at startup, say --
        // would cut off part-way down the curve, at an audible step.
        let fade = Fade { curve, frames: (kept / ch) as u64, fade_in: false };
        {
            let (front, back) = s.ring.as_mut_slices();
            fade.apply(front, ch, 0);
            let wrapped_at = (front.len() / ch) as u64;
            fade.apply(back, ch, wrapped_at);
        }
        let placed = s.ring.mix_at(lead_samples, overlay);
        (kept, placed)
    }

    pub fn buffered(&self) -> usize {
        self.state.lock().map(|s| s.ring.len()).unwrap_or(0)
    }

    /// Names of available output devices, for diagnosing a failed match.
    pub fn list_devices() -> Vec<String> {
        cpal::default_host()
            .output_devices()
            .map(|ds| ds.filter_map(|d| d.name().ok()).collect())
            .unwrap_or_default()
    }

    pub fn diagnostics(&self) -> (u64, u64) {
        self.state
            .lock()
            .map(|s| (s.underrun_samples, s.lock_failures))
            .unwrap_or((0, 0))
    }
}

/// The whole of the real-time path.
///
/// `try_lock` rather than `lock`: blocking here would stall the audio device.
/// On contention we emit silence and count it, because a glitch that is
/// recorded can be fixed and a glitch that is hidden cannot.
fn fill(state: &Arc<Mutex<OutputState>>, volume: &Volume, out: &mut [f32]) {
    match state.try_lock() {
        Ok(mut s) => {
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
                s.underrun_samples += (out.len() - got) as u64;
            }
        }
        Err(_) => {
            out.iter_mut().for_each(|v| *v = 0.0);
            if let Ok(mut s) = state.try_lock() {
                s.lock_failures += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The real-time path is testable without a device: `fill` is a plain
    // function over shared state, which is why it was written that way.
    #[test]
    fn fill_pads_with_silence_and_counts_the_shortfall() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[0.5; 4]);
        let mut out = [9.9f32; 8];
        fill(&st, &Volume::new(1.0), &mut out);
        assert_eq!(&out[..4], &[0.5; 4]);
        assert!(out[4..].iter().all(|v| *v == 0.0), "shortfall must be silence");
        assert_eq!(st.lock().unwrap().underrun_samples, 4);
    }

    /// Volume is applied at the DEVICE, not before submission. The ring holds
    /// ~14 s, so scaling on the way in makes a change inaudible until that much
    /// already-scaled audio has drained [REQ-AUD-152].
    #[test]
    fn fill_applies_volume_at_the_callback() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[1.0; 4]);
        let mut out = [0.0f32; 4];
        fill(&st, &Volume::new(0.25), &mut out);
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
        fill(&st, &vol, &mut a);
        assert!((a[0] - 1.0).abs() < 1e-6);
        vol.set(0.5); // turned down while the rest is still queued
        let mut b = [0.0f32; 4];
        fill(&st, &vol, &mut b);
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
        fill(&st, &Volume::new(1.0), &mut out);
        assert_eq!(st.lock().unwrap().underrun_samples, 0);
    }

    #[test]
    fn fill_emits_silence_rather_than_blocking_when_locked() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        let _held = st.lock().unwrap();
        let mut out = [9.9f32; 4];
        fill(&st, &Volume::new(1.0), &mut out); // must return, not deadlock
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
