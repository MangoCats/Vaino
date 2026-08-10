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

pub struct Output {
    // Held to keep the stream alive; dropping it stops playback.
    _stream: cpal::Stream,
    pub state: Arc<Mutex<OutputState>>,
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
        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                &config,
                move |out: &mut [f32], _| fill(&cb_state, out),
                err_fn,
                None,
            ),
            SampleFormat::I16 => {
                let mut scratch: Vec<f32> = Vec::new();
                device.build_output_stream(
                    &config,
                    move |out: &mut [i16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &mut scratch);
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
                device.build_output_stream(
                    &config,
                    move |out: &mut [u16], _| {
                        scratch.resize(out.len(), 0.0);
                        fill(&cb_state, &mut scratch);
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
        Ok(Self { _stream: stream, state, sample_rate, channels, device_name })
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
fn fill(state: &Arc<Mutex<OutputState>>, out: &mut [f32]) {
    match state.try_lock() {
        Ok(mut s) => {
            let got = s.ring.read(out);
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
        fill(&st, &mut out);
        assert_eq!(&out[..4], &[0.5; 4]);
        assert!(out[4..].iter().all(|v| *v == 0.0), "shortfall must be silence");
        assert_eq!(st.lock().unwrap().underrun_samples, 4);
    }

    #[test]
    fn fill_reports_no_underrun_when_supplied() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        st.lock().unwrap().ring.write(&[0.25; 8]);
        let mut out = [0.0f32; 8];
        fill(&st, &mut out);
        assert_eq!(st.lock().unwrap().underrun_samples, 0);
    }

    #[test]
    fn fill_emits_silence_rather_than_blocking_when_locked() {
        let st = Arc::new(Mutex::new(OutputState::new(16)));
        let _held = st.lock().unwrap();
        let mut out = [9.9f32; 4];
        fill(&st, &mut out); // must return, not deadlock
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
