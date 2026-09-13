//! Echo playback: the anchor arithmetic, and the things that invalidate it.
//!
//! Phase 3 of [GUIDE009], the parts `[GDE-ECHO-370]` says to unit-test rather
//! than listen to: "the anchor arithmetic, the hysteresis and every row of
//! `[GDE-ECHO-360]` are unit-testable against a synthetic clock that can be
//! told to run fast -- and should be, because they are the parts where a sign
//! error is invisible in listening and obvious in a test."
//!
//! **No transport here, deliberately.** `[GDE-ECHO-320]` makes every message
//! absolute, idempotent and independently sufficient, which is what renders
//! the transport choice uninteresting: a node that misses one uses the next,
//! and a node that receives a stale one discards it by its own timestamp.
//! Nothing in this module needs to know how the bytes arrived.
//!
//! The two messages are [`Schedule`] (forward, emitted on mixer admission,
//! ~15 s before anyone hears it) and [`DriftAnchor`] (backward, repeated).
//! One kind is not enough `[GDE-ECHO-310]`: a backward-looking statement
//! cannot serve a node whose presentation offset exceeds the master's, because
//! such a node needed to submit *before the master did* and would learn of it
//! too late to act.

use std::time::Duration;

/// Nanoseconds on the chrony-disciplined wall clock, since the epoch.
///
/// The same basis `FrameClock::sample` pairs its frame count with. Deliberately
/// **not** cpal's `StreamInstant`, which counts from each stream's own trigger
/// and so shares no epoch between machines `[GDE-ECHO-160]`.
pub type WallNanos = u64;

/// What a node has to know about itself to place a schedule.
///
/// `[GDE-ECHO-410]`'s model, both halves. Measured per node, not assumed:
/// `bose` 2043 frames and `vainopi` 15676 as of `[LOG-CPAL-060]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeTiming {
    /// Submit-to-air delay, in frames. ALSA's reported delay plus whatever
    /// calibrated residual the node has `[GDE-ECHO-430]`.
    pub presentation_offset_frames: u64,
    /// The device's nominal rate. Frames are converted at this; the *actual*
    /// rate error is what the trim loop corrects, and does not belong here.
    pub rate: u32,
}

impl NodeTiming {
    pub fn offset(&self) -> Duration {
        frames_to_duration(self.presentation_offset_frames, self.rate)
    }
}

/// Forward: *passage P, sample 0, will be heard at wall time T.*
///
/// Emitted when the master admits P to the mixer, which by `[REQ-AUD-160]` is
/// roughly 15 s before anyone hears it. That lead is the whole reason an
/// arbitrary presentation offset is compensable, and it exists only because an
/// echo node holds the file locally and knows the queue `[GDE-ECHO-420]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Schedule {
    pub passage_id: i64,
    /// When sample 0 reaches the **air**, not the device.
    pub sound_at: WallNanos,
    pub rate: u32,
}

/// Backward: *sample N of P was heard at T, and my rate error is this.*
///
/// Computed from `audible_ms`, never `played_ms`: those differ by the ring's
/// depth and only one of them describes sound `[REQ-AUD-164]`. Note that the
/// engine's own `audible_ms` subtracts the ring but **not** the device delay,
/// which is imperceptible for a display and is not for `vainopi`'s 355 ms --
/// so an anchor must subtract [`NodeTiming::offset`] as well.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriftAnchor {
    pub passage_id: i64,
    pub sample: u64,
    pub heard_at: WallNanos,
    pub rate: u32,
    /// The master's own measured rate error, parts per million, signed.
    pub ppm: f64,
}

fn frames_to_duration(frames: u64, rate: u32) -> Duration {
    Duration::from_nanos(frames.saturating_mul(1_000_000_000) / rate.max(1) as u64)
}

/// When this node must **submit** sample 0 for it to be heard on time.
///
/// `[GDE-ECHO-410]`: `submit_time(node, N) = sound_time(N) - offset(node)`.
/// Symmetric by construction -- no node is the reference, and the master
/// applies the identical arithmetic to itself. A design that instead treated
/// the master's offset as zero would have worked in one direction only.
///
/// `None` when the schedule has already passed for this node: with an offset
/// of 355 ms and an anchor 100 ms in the future, submission was due 255 ms
/// ago, and there is no arithmetic that recovers it. Saying so beats returning
/// a time in the past that reads like an instruction `[GOV-SRC-040]`.
pub fn submit_at(sched: &Schedule, node: NodeTiming, now: WallNanos) -> Option<WallNanos> {
    let offset_ns = node.offset().as_nanos() as u64;
    let submit = sched.sound_at.checked_sub(offset_ns)?;
    if submit < now {
        return None;
    }
    Some(submit)
}

/// How far this node is from the master, in nanoseconds, for the same sample.
///
/// Positive means **this node is late** -- its audio reached the air after the
/// master's did, so it must speed up or start earlier. The sign is stated here
/// because it is the one a test catches and listening does not.
pub fn residual_ns(anchor: &DriftAnchor, local_heard_at: WallNanos) -> i64 {
    local_heard_at as i64 - anchor.heard_at as i64
}

/// Where a passage actually is **in the air**, and when that was true.
///
/// Distinct from the engine's `audible_ms`, which subtracts the output ring
/// but not the device: those differ by the presentation offset, which is 46 ms
/// on `bose` and 355 on `vainopi` `[LOG-CPAL-060]`. Imperceptible for a
/// display, and a third of a second for echo.
///
/// `audible_ms` is deliberately **not** changed to match. It drives the UI and
/// the resume point, and moving those by 355 ms to serve echo would be the
/// tail wagging the dog `[REQ-AUD-164]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirPosition {
    pub passage_id: i64,
    pub position_ms: u64,
    pub at: WallNanos,
}

/// What is being heard, from what the mixer has produced and what is queued
/// ahead of the air.
///
/// Saturates at zero rather than wrapping: early in a passage the ring plus
/// the device delay exceed what has been mixed, and the honest answer is that
/// none of this passage is audible yet.
pub fn air_position(
    passage_id: i64,
    played_ms: u64,
    ring_frames: u64,
    device_delay_frames: u64,
    rate: u32,
    at: WallNanos,
) -> AirPosition {
    let ahead_ms = (ring_frames + device_delay_frames) * 1000 / rate.max(1) as u64;
    AirPosition { passage_id, position_ms: played_ms.saturating_sub(ahead_ms), at }
}

impl AirPosition {
    /// The backward anchor this position supports.
    pub fn anchor(&self, rate: u32, ppm: f64) -> DriftAnchor {
        DriftAnchor {
            passage_id: self.passage_id,
            sample: self.position_ms * rate as u64 / 1000,
            heard_at: self.at,
            rate,
            ppm,
        }
    }
}

/// When a passage admitted to the mixer **now** will start to sound.
///
/// Everything already in the ring plays first, then the device's own delay.
/// That sum is the ~15 s of lead `[REQ-AUD-160]` gives, and it is the entire
/// reason an arbitrary presentation offset is compensable `[GDE-ECHO-310]`:
/// the announcement goes out long before anybody could hear it.
pub fn schedule_for_admission(
    passage_id: i64,
    ring_frames: u64,
    device_delay_frames: u64,
    rate: u32,
    now: WallNanos,
) -> Schedule {
    let ahead_ns = (ring_frames + device_delay_frames) * 1_000_000_000 / rate.max(1) as u64;
    Schedule { passage_id, sound_at: now + ahead_ns, rate }
}

/// Everything that voids the frame clock as a basis for an anchor.
///
/// `[GDE-ECHO-360]`. Each of these must force a rejoin at the next passage
/// boundary rather than a silent continuation on stale state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voided {
    /// Frame counter and stream epoch both reset `[SPEC-APS-010]`.
    DeviceReopen,
    /// Frames that were counted were never heard `[REQ-AUD-142]`.
    Underrun,
    /// The device stopped and the count stopped with it.
    Pause,
    /// The ring was cut `[REQ-AUD-158]`, so counted frames were discarded.
    Skip,
}

/// Whether the local frame clock can still be compared against an anchor.
///
/// **The derived default is the invalid state, and that is the point.** A node
/// that has just opened a stream has not yet earned the right to be compared
/// against anything, so `established` starts false and only a clean passage
/// boundary sets it. `Undetermined` is not `Valid` `[GOV-SRC-040]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Basis {
    voided_by: Option<Voided>,
    established: bool,
}

impl Basis {
    /// A passage boundary reached cleanly: this is the only way in.
    pub fn establish(&mut self) {
        self.voided_by = None;
        self.established = true;
    }
    pub fn void(&mut self, why: Voided) {
        self.voided_by = Some(why);
        self.established = false;
    }
    pub fn is_valid(&self) -> bool {
        self.established && self.voided_by.is_none()
    }
    /// What broke it, for saying so rather than silently not correcting.
    pub fn voided_by(&self) -> Option<Voided> {
        self.voided_by
    }
}

/// One correction step for the rate error `[GDE-ECHO-340]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trim {
    /// Inside the deadband, or too soon, or on no basis at all.
    None,
    /// This node is **late**: skip a frame so its playback position advances
    /// without time passing, and it catches up.
    DropFrame,
    /// This node is **early**: repeat a frame so time passes without its
    /// position advancing, and it falls back.
    DuplicateFrame,
}

/// Whether to trim, given the residual and how long since the last trim.
///
/// `[GDE-ECHO-350]`: "Trim only while the estimated offset exceeds a deadband
/// comfortably larger than the measurement noise, never more than one frame
/// per correction interval, and never on an estimate younger than the
/// regression window. A correction loop that reacts to its own measurement
/// noise produces exactly the slow periodic wobble it was built to remove."
///
/// All three guards are parameters rather than constants because the numbers
/// are still being measured: `vainopi` has no single rate at all
/// `[LOG-CAL-080]`, so a constant chosen today would be wrong for it tomorrow.
pub fn trim_decision(
    basis: &Basis,
    residual: i64,
    deadband: Duration,
    since_last_trim: Duration,
    min_interval: Duration,
) -> Trim {
    if !basis.is_valid() {
        return Trim::None;
    }
    if since_last_trim < min_interval {
        return Trim::None;
    }
    let band = deadband.as_nanos() as i64;
    if residual.abs() <= band {
        return Trim::None;
    }
    // Positive residual means this node is LATE, so it must catch up: drop a
    // frame, and the position advances by one more than the time spent.
    // Getting this backwards doubles the error instead of removing it, and
    // sounds like nothing in particular until hours have passed -- which is
    // why `trim_directions_are_not_reversed` exists and why the mutation that
    // swaps these two arms fails three tests.
    if residual > 0 {
        Trim::DropFrame
    } else {
        Trim::DuplicateFrame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOSE: NodeTiming = NodeTiming { presentation_offset_frames: 2043, rate: 44100 };
    const VAINOPI: NodeTiming = NodeTiming { presentation_offset_frames: 15676, rate: 44100 };

    const SEC: u64 = 1_000_000_000;

    #[test]
    fn offsets_match_the_measured_nodes() {
        // `[LOG-CPAL-060]`: 46.3 ms and 355 ms.
        assert_eq!(BOSE.offset().as_millis(), 46);
        assert_eq!(VAINOPI.offset().as_millis(), 355);
    }

    #[test]
    fn the_node_with_the_larger_offset_submits_earlier() {
        // The whole point of `[GDE-ECHO-410]`. A backward-only design would
        // have had vainopi submitting after bose and never catching up.
        let sched = Schedule { passage_id: 7, sound_at: 100 * SEC, rate: 44100 };
        let now = 80 * SEC;
        let b = submit_at(&sched, BOSE, now).unwrap();
        let v = submit_at(&sched, VAINOPI, now).unwrap();
        assert!(v < b, "vainopi must submit before bose");
        assert_eq!(b - v, (VAINOPI.offset() - BOSE.offset()).as_nanos() as u64);
        // 355 - 46 = 309 ms, the figure the design turns on.
        assert_eq!((b - v) / 1_000_000, 309);
    }

    #[test]
    fn the_master_applies_the_same_arithmetic_to_itself() {
        // Symmetry: no node is the reference `[GDE-ECHO-410]`.
        let sched = Schedule { passage_id: 1, sound_at: 50 * SEC, rate: 44100 };
        let submit = submit_at(&sched, BOSE, 0).unwrap();
        assert_eq!(submit, 50 * SEC - BOSE.offset().as_nanos() as u64);
    }

    #[test]
    fn a_schedule_already_past_is_refused_not_rounded_forward() {
        // 100 ms of lead against a 355 ms offset: submission was due 255 ms
        // ago. Returning a past instant would read like an instruction.
        let sched = Schedule { passage_id: 2, sound_at: 10 * SEC + 100_000_000, rate: 44100 };
        assert!(submit_at(&sched, VAINOPI, 10 * SEC).is_none());
        // The same schedule is comfortably reachable by the low-offset node.
        assert!(submit_at(&sched, BOSE, 10 * SEC).is_some());
    }

    #[test]
    fn fifteen_seconds_of_lead_clears_every_measured_offset() {
        // `[GDE-ECHO-310]`'s claim, checked rather than asserted.
        let now = 1000 * SEC;
        let sched = Schedule { passage_id: 3, sound_at: now + 15 * SEC, rate: 44100 };
        for node in [BOSE, VAINOPI] {
            assert!(submit_at(&sched, node, now).is_some());
        }
    }

    #[test]
    fn residual_sign_says_late_is_positive() {
        let a = DriftAnchor {
            passage_id: 4, sample: 44100, heard_at: 5 * SEC, rate: 44100, ppm: 0.0,
        };
        assert!(residual_ns(&a, 5 * SEC + 1_000_000) > 0, "later than master is positive");
        assert!(residual_ns(&a, 5 * SEC - 1_000_000) < 0, "earlier than master is negative");
        assert_eq!(residual_ns(&a, 5 * SEC), 0);
    }

    // `[REQ-AUD-160]`'s ring is ~15 s at 44100.
    const RING: u64 = 44100 * 15;

    #[test]
    fn the_air_lags_the_mixer_by_the_ring_and_the_device() {
        let a = air_position(9, 30_000, RING, BOSE.presentation_offset_frames, 44100, 7 * SEC);
        // 30 s mixed, less 15 s of ring and 46 ms of device.
        assert_eq!(a.position_ms, 30_000 - 15_000 - 46);
        assert_eq!(a.at, 7 * SEC);
    }

    #[test]
    fn the_device_delay_is_what_separates_air_from_audible_ms() {
        // The engine's audible_ms subtracts the ring only. On bose that is a
        // 46 ms difference and on vainopi 355 -- one is a rounding error in a
        // display, the other is not `[LOG-CPAL-060]`.
        let b = air_position(1, 60_000, RING, BOSE.presentation_offset_frames, 44100, 0);
        let v = air_position(1, 60_000, RING, VAINOPI.presentation_offset_frames, 44100, 0);
        assert_eq!(b.position_ms - v.position_ms, 355 - 46);
    }

    #[test]
    fn early_in_a_passage_nothing_is_audible_yet_rather_than_negative() {
        // 2 s mixed against 15 s of ring: saturates, does not wrap.
        let a = air_position(2, 2_000, RING, VAINOPI.presentation_offset_frames, 44100, 0);
        assert_eq!(a.position_ms, 0);
    }

    #[test]
    fn an_admission_is_announced_about_a_ring_ahead_of_being_heard() {
        let now = 500 * SEC;
        let s = schedule_for_admission(5, RING, BOSE.presentation_offset_frames, 44100, now);
        let lead_ms = (s.sound_at - now) / 1_000_000;
        assert!((15_000..=15_100).contains(&lead_ms), "lead was {lead_ms} ms");
        // And that lead is what makes every node's offset compensable.
        for node in [BOSE, VAINOPI] {
            assert!(submit_at(&s, node, now).is_some());
        }
    }

    #[test]
    fn an_anchor_round_trips_through_the_sample_number() {
        let a = air_position(3, 10_000, 0, 0, 44100, 12 * SEC);
        let anchor = a.anchor(44100, -2.09);
        assert_eq!(anchor.sample, 441_000);           // 10 s at 44100
        assert_eq!(anchor.heard_at, 12 * SEC);
        assert_eq!(residual_ns(&anchor, 12 * SEC), 0);
    }

    #[test]
    fn two_nodes_agreeing_on_the_air_have_no_residual() {
        // The property echo is trying to hold: different offsets, same sound.
        let sched = Schedule { passage_id: 8, sound_at: 900 * SEC, rate: 44100 };
        let now = 880 * SEC;
        let b = submit_at(&sched, BOSE, now).unwrap() + BOSE.offset().as_nanos() as u64;
        let v = submit_at(&sched, VAINOPI, now).unwrap() + VAINOPI.offset().as_nanos() as u64;
        assert_eq!(b, v, "submit + own offset must land on the same instant");
        assert_eq!(b, sched.sound_at);
    }

    #[test]
    fn a_fresh_basis_is_not_valid_until_a_clean_boundary() {
        let mut b = Basis::default();
        assert!(!b.is_valid(), "absent is not zero `[GOV-SRC-040]`");
        b.establish();
        assert!(b.is_valid());
    }

    #[test]
    fn every_row_of_the_invalidation_table_voids_the_basis() {
        for why in [Voided::DeviceReopen, Voided::Underrun, Voided::Pause, Voided::Skip] {
            let mut b = Basis::default();
            b.establish();
            assert!(b.is_valid());
            b.void(why);
            assert!(!b.is_valid(), "{why:?} must void the basis");
            assert_eq!(b.voided_by(), Some(why), "and must say which");
            // Only a clean boundary re-establishes it.
            b.establish();
            assert!(b.is_valid());
        }
    }

    #[test]
    fn no_trim_without_a_basis_however_large_the_error() {
        let b = Basis::default();
        let t = trim_decision(&b, 10 * SEC as i64, Duration::from_micros(500),
                              Duration::from_secs(60), Duration::from_secs(1));
        assert_eq!(t, Trim::None, "a huge residual on stale state is not a licence");
    }

    #[test]
    fn trim_directions_are_not_reversed() {
        let mut b = Basis::default();
        b.establish();
        let band = Duration::from_micros(500);
        let (elapsed, interval) = (Duration::from_secs(10), Duration::from_secs(1));
        // Late -> drop, so the node stops falling further behind.
        assert_eq!(trim_decision(&b, 2_000_000, band, elapsed, interval), Trim::DropFrame);
        // Early -> duplicate.
        assert_eq!(trim_decision(&b, -2_000_000, band, elapsed, interval), Trim::DuplicateFrame);
    }

    #[test]
    fn the_deadband_holds_and_is_inclusive_at_its_edge() {
        let mut b = Basis::default();
        b.establish();
        let band = Duration::from_micros(500);
        let (elapsed, interval) = (Duration::from_secs(10), Duration::from_secs(1));
        assert_eq!(trim_decision(&b, 500_000, band, elapsed, interval), Trim::None);
        assert_eq!(trim_decision(&b, -500_000, band, elapsed, interval), Trim::None);
        assert_eq!(trim_decision(&b, 500_001, band, elapsed, interval), Trim::DropFrame);
    }

    #[test]
    fn the_rate_limit_holds_even_far_outside_the_deadband() {
        let mut b = Basis::default();
        b.establish();
        let t = trim_decision(&b, 50_000_000, Duration::from_micros(500),
                              Duration::from_millis(100), Duration::from_secs(1));
        assert_eq!(t, Trim::None, "never more than one frame per interval");
    }

    #[test]
    fn a_loop_fed_pure_noise_inside_the_band_never_trims() {
        // `[GDE-ECHO-350]`'s failure mode: a correction loop reacting to its
        // own measurement noise produces the slow wobble it exists to remove.
        let mut b = Basis::default();
        b.establish();
        let band = Duration::from_micros(500);
        let mut state: i64 = 12345;
        for _ in 0..10_000 {
            state = (state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407)) >> 1;
            let noise = state % 500_000; // strictly inside the band
            assert_eq!(
                trim_decision(&b, noise, band, Duration::from_secs(5), Duration::from_secs(1)),
                Trim::None
            );
        }
    }

    #[test]
    fn a_synthetic_node_running_fast_is_driven_back_to_the_band() {
        // The closed loop, against a clock told to run fast `[GDE-ECHO-370]`.
        // 14 ppm is bose's measured error `[LOG-FIX-030]`.
        let mut b = Basis::default();
        b.establish();
        let band = Duration::from_micros(500);
        let interval = Duration::from_secs(1);
        let rate = 44100i64;
        let ppm = 14.0f64;
        let mut residual: i64 = 5_000_000; // 5 ms late to begin with
        let mut trims = 0;
        for _ in 0..20_000 {
            // One second passes: the node's own error accrues...
            residual += (ppm * 1000.0) as i64; // ppm -> ns per second
            // ...and at most one frame is trimmed against it.
            match trim_decision(&b, residual, band, interval, interval) {
                Trim::DropFrame => { residual -= 1_000_000_000 / rate; trims += 1; }
                Trim::DuplicateFrame => { residual += 1_000_000_000 / rate; trims += 1; }
                Trim::None => {}
            }
        }
        assert!(trims > 0, "the loop must actually act");
        assert!(
            residual.abs() <= band.as_nanos() as i64 + 1_000_000_000 / rate,
            "converged to within the deadband plus one frame, got {residual} ns"
        );
    }
}
