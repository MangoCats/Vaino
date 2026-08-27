//! Fade curves. **The only place in the player where fade math lives.**
//!
//! Per `[XFD-ORTH-020]`, the mixer never computes a curve -- it sums audio to
//! which fades have already been applied. Keeping every curve here means a
//! crossfade bug has exactly one place to be.
//!
//! Fade-out is not a separate family of curves: it is a fade-in evaluated on
//! remaining progress. `[XFD-CURV-020]` lists exponential fade-in and
//! `[XFD-CURV-030]` logarithmic fade-out, and for a linear-in-decibels fade
//! those are the same function mirrored --
//!
//! ```text
//!   fade-in   g(t) = 10^(-D(1-t)/20)      slow start, fast finish
//!   fade-out  g(t) = g_in(1-t) = 10^(-Dt/20)   fast start, slow finish
//! ```
//!
//! so one function and a mirrored argument covers both, and the two can never
//! drift apart.

/// Fade shape. Named for the fade-**in** sense; see [`Curve::gain_out`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// Constant rate of change. Precise and predictable `[XFD-LIN-010]`.
    Linear,
    /// Smooth acceleration and deceleration. Gentle and musical `[XFD-COS-010]`.
    Cosine,
    /// Linear in decibels: slow start, fast finish on the way in; fast start,
    /// slow finish on the way out `[XFD-EXP-010]`, `[XFD-EXP-020]`.
    Exponential,
}

/// Depth of the exponential curve, in dB below unity at the silent end.
/// 60 dB is the conventional floor for a musical fade.
const EXP_DEPTH_DB: f32 = 60.0;

impl Curve {
    /// Gain for a fade-**in** at normalised progress `t`, clamped to `[0, 1]`.
    ///
    /// `gain_in(0.0) == 0.0` and `gain_in(1.0) == 1.0` for every curve, so a
    /// fade always starts silent and ends at unity regardless of shape.
    pub fn gain_in(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Curve::Linear => t,
            Curve::Cosine => (1.0 - (std::f32::consts::PI * t).cos()) * 0.5,
            // 10^(-D(1-t)/20), shifted so t == 0 is exactly silent rather than
            // -60 dB. Without the shift a "fade in" would begin audibly.
            Curve::Exponential => {
                let floor = 10f32.powf(-EXP_DEPTH_DB / 20.0);
                let raw = 10f32.powf(-EXP_DEPTH_DB * (1.0 - t) / 20.0);
                ((raw - floor) / (1.0 - floor)).clamp(0.0, 1.0)
            }
        }
    }

    /// Gain for a fade-**out** at normalised progress `t`.
    ///
    /// Defined as the mirror of [`Curve::gain_in`], which is why `Exponential`
    /// serves as the logarithmic fade-out of `[XFD-EXP-020]`.
    pub fn gain_out(self, t: f32) -> f32 {
        self.gain_in(1.0 - t)
    }
}

/// A fade applied over a span of frames.
///
/// Holds no audio and no curve state beyond a counter, so it costs nothing to
/// keep one per passage.
#[derive(Debug, Clone, Copy)]
pub struct Fade {
    pub curve: Curve,
    /// Length of the fade. Zero means pass-through at unity gain
    /// `[XFD-OV-010]`: "Fade Duration = 0 ... no fade curve applied".
    pub frames: u64,
    pub fade_in: bool,
}

impl Fade {
    pub fn none() -> Self {
        Fade { curve: Curve::Linear, frames: 0, fade_in: true }
    }

    /// Gain at `frame` frames into the fade.
    pub fn gain_at(&self, frame: u64) -> f32 {
        if self.frames == 0 {
            return 1.0; // pass-through
        }
        let t = (frame as f32 / self.frames as f32).clamp(0.0, 1.0);
        if self.fade_in { self.curve.gain_in(t) } else { self.curve.gain_out(t) }
    }

    /// Apply the fade in place to interleaved frames starting at `start_frame`.
    ///
    /// This is the single point at which gain touches audio; the mixer
    /// downstream only ever adds `[XFD-ORTH-020]`.
    pub fn apply(&self, samples: &mut [f32], channels: usize, start_frame: u64) {
        if self.frames == 0 || channels == 0 {
            return;
        }
        for (i, frame) in samples.chunks_mut(channels).enumerate() {
            let g = self.gain_at(start_frame + i as u64);
            frame.iter_mut().for_each(|s| *s *= g);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVES: [Curve; 3] = [Curve::Linear, Curve::Cosine, Curve::Exponential];

    #[test]
    fn every_curve_spans_silence_to_unity() {
        for c in CURVES {
            assert!(c.gain_in(0.0).abs() < 1e-6, "{c:?} must start silent");
            assert!((c.gain_in(1.0) - 1.0).abs() < 1e-6, "{c:?} must end at unity");
        }
    }

    #[test]
    fn every_curve_is_monotonic() {
        for c in CURVES {
            let mut prev = -1.0;
            for i in 0..=100 {
                let g = c.gain_in(i as f32 / 100.0);
                assert!(g >= prev - 1e-6, "{c:?} dipped at t={}", i as f32 / 100.0);
                prev = g;
            }
        }
    }

    /// The property that lets one function serve both directions.
    #[test]
    fn fade_out_mirrors_fade_in() {
        for c in CURVES {
            for i in 0..=100 {
                let t = i as f32 / 100.0;
                assert!((c.gain_out(t) - c.gain_in(1.0 - t)).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn exponential_falls_faster_than_linear_early_in_a_fade_out() {
        // "fast start, slow finish" [XFD-EXP-020]
        let t = 0.15;
        assert!(Curve::Exponential.gain_out(t) < Curve::Linear.gain_out(t));
    }

    /// The one guard against the waveform editor's preview lying about what
    /// production plays `[SPEC021 §4]`: `fade.js` is checked against the same
    /// `fixtures/fade/exponential.json` table this test checks Rust against.
    #[test]
    fn matches_the_shared_fade_fixture_js_is_also_checked_against() {
        let json = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../fixtures/fade/exponential.json"
        ));
        let rows: Vec<serde_json::Value> = serde_json::from_str(json).unwrap();
        assert!(rows.len() >= 10, "the fixture must not have shrunk to nothing");
        for row in rows {
            let t = row["t"].as_f64().unwrap() as f32;
            let want_in = row["gain_in"].as_f64().unwrap() as f32;
            let want_out = row["gain_out"].as_f64().unwrap() as f32;
            let got_in = Curve::Exponential.gain_in(t);
            let got_out = Curve::Exponential.gain_out(t);
            assert!((got_in - want_in).abs() < 1e-5, "gain_in({t}): got {got_in}, want {want_in}");
            assert!(
                (got_out - want_out).abs() < 1e-5,
                "gain_out({t}): got {got_out}, want {want_out}"
            );
        }
    }

    #[test]
    fn zero_length_fade_is_pass_through() {
        let f = Fade::none();
        assert_eq!(f.gain_at(0), 1.0);
        assert_eq!(f.gain_at(99_999), 1.0);
        let mut buf = vec![0.5f32; 8];
        f.apply(&mut buf, 2, 0);
        assert!(buf.iter().all(|s| (*s - 0.5).abs() < 1e-9));
    }

    #[test]
    fn apply_scales_both_channels_identically() {
        let f = Fade { curve: Curve::Linear, frames: 4, fade_in: true };
        let mut buf = vec![1.0f32; 8]; // 4 frames, stereo
        f.apply(&mut buf, 2, 0);
        for frame in buf.chunks(2) {
            assert!((frame[0] - frame[1]).abs() < 1e-9, "channels must match");
        }
        assert!(buf[0] < buf[2] && buf[2] < buf[4], "gain must rise across frames");
    }
}
