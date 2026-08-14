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

/// How much range the fader spans, in dB `[REQ-AUD-154]`.
///
/// MuLibPlay's figure, from six years of daily use: it ran a `-8192..=0`
/// integer slider scaled by 1/128 into exactly this curve. Wider than 64 dB
/// and the bottom of the travel is inaudible anyway; much narrower and the
/// quietest usable setting is not quiet enough for a room at night.
pub const FADER_DB: f32 = 64.0;

impl Volume {
    /// Fader travel (0.0 at the bottom, 1.0 at the top) to amplitude.
    ///
    /// Equal travel gives an equal change in *decibels*, because that is what
    /// the ear hears as an even change in loudness. A linear fader spends its
    /// top half on differences barely distinguishable from full and its bottom
    /// half plunging to silence -- the taper we started with, and audibly
    /// wrong at both ends.
    ///
    /// The very bottom is silence, not -64 dB: a fader that cannot be closed
    /// is a fault, and -64 dB is quiet but not nothing.
    pub fn amplitude_at(travel: f32) -> f32 {
        let t = travel.clamp(0.0, 1.0);
        if t <= 0.0 {
            return 0.0;
        }
        10f32.powf((t - 1.0) * FADER_DB / 20.0)
    }

    /// The inverse, for putting the knob back where the listener left it.
    /// The stored value is amplitude, so the UI has to be told a position.
    pub fn travel_for(amplitude: f32) -> f32 {
        let a = amplitude.clamp(0.0, 1.0);
        if a <= 0.0 {
            return 0.0;
        }
        (1.0 + a.log10() * 20.0 / FADER_DB).clamp(0.0, 1.0)
    }

    /// Level in dB relative to full scale, or `None` when closed -- which is
    /// negative infinity, and not a number a display should try to render.
    pub fn db_at(travel: f32) -> Option<f32> {
        let t = travel.clamp(0.0, 1.0);
        (t > 0.0).then(|| (t - 1.0) * FADER_DB)
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

    /// The ends must be exact: full scale must not be 0.999, and the bottom
    /// must be true silence rather than something merely quiet.
    #[test]
    fn fader_ends_are_exact() {
        assert_eq!(Volume::amplitude_at(1.0), 1.0);
        assert_eq!(Volume::amplitude_at(0.0), 0.0);
        assert_eq!(Volume::db_at(1.0), Some(0.0));
        assert_eq!(Volume::db_at(0.0), None, "a closed fader has no dB value");
    }

    /// Equal travel, equal dB -- the property that makes the taper worth
    /// having. Half travel is half the dB range down, NOT half the amplitude.
    #[test]
    fn fader_is_even_in_decibels() {
        for (travel, want_db) in [(0.75, -16.0), (0.5, -32.0), (0.25, -48.0)] {
            let got = 20.0 * Volume::amplitude_at(travel).log10();
            assert!((got - want_db).abs() < 1e-3, "at {travel}: {got} dB, want {want_db}");
        }
        // The step from 0.75 to 0.5 must equal the step from 0.5 to 0.25.
        let db = |t: f32| 20.0 * Volume::amplitude_at(t).log10();
        assert!(((db(0.75) - db(0.5)) - (db(0.5) - db(0.25))).abs() < 1e-3);
    }

    /// A linear fader would put half travel at -6 dB, which is the fault being
    /// corrected: barely quieter, with the whole audible range crammed into
    /// the last sliver of movement.
    #[test]
    fn half_travel_is_not_half_amplitude() {
        assert!(Volume::amplitude_at(0.5) < 0.05, "{}", Volume::amplitude_at(0.5));
    }

    /// The knob has to go back where the listener left it after a restart.
    #[test]
    fn travel_round_trips_through_amplitude() {
        for step in 0..=100 {
            let t = step as f32 / 100.0;
            let back = Volume::travel_for(Volume::amplitude_at(t));
            assert!((back - t).abs() < 1e-4, "travel {t} came back as {back}");
        }
    }

    #[test]
    fn fader_is_monotonic() {
        let mut prev = -1.0;
        for step in 0..=100 {
            let a = Volume::amplitude_at(step as f32 / 100.0);
            assert!(a > prev, "not increasing at step {step}");
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
