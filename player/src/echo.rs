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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Schedule {
    pub passage_id: i64,
    /// The sample within the passage that reaches the air at `sound_at`.
    ///
    /// Zero for an ordinary passage-to-passage transition, which every node
    /// sees coming and holds sync straight through `[GDE-ECHO-325]`. Non-zero
    /// is a **seek**: the master announces *this passage, this far in, at this
    /// instant* and each node fills toward that target independently. Carrying
    /// the sample here rather than inventing a second message is what keeps a
    /// seek from being a special case -- it is the same schedule with a
    /// different offset and a nearer `sound_at`.
    ///
    /// Defaulted on the wire, and this is the one place where absent really
    /// does mean zero `[GOV-SRC-040]`: a master too old to publish the field
    /// is a master that could only ever announce passage starts, so every
    /// schedule it ever sent began at sample 0. Reading one as such is not a
    /// guess, it is what the sender meant.
    #[serde(default)]
    pub start_sample: u64,
    /// When `start_sample` reaches the **air**, not the device.
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
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DriftAnchor {
    pub passage_id: i64,
    pub sample: u64,
    pub heard_at: WallNanos,
    pub rate: u32,
    /// The master's own measured rate error, parts per million, signed.
    ///
    /// `None` when nothing has measured it, which is **not** the same claim as
    /// `Some(0.0)` `[GOV-SRC-040]`. A follower reading a bare `+0.00` would
    /// reasonably take it for "the master checked and found no error"; this
    /// makes "nobody has computed this" a thing the wire can say. Caught by
    /// the first client that ever read one.
    pub ppm: Option<f64>,
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

/// Where sample 0 must sit in this node's output ring at admission.
///
/// **`submit_at` says *when* sample 0 must reach the device; it does not say
/// how a node makes that happen, and the obvious reading is wrong.** Admitting
/// the passage to the mixer at `submit_at` puts sample 0 at the *back* of a
/// ring that is 15.0 s deep `[LOG-ECHO-020]`, so it would sound a full ring
/// late. The knob is not when to admit -- it is how much audio sits ahead of
/// sample 0 when it does.
///
/// In steady state admission is not a free choice anyway: the ring is full of
/// the previous passage, and sample 0 goes in where that passage ends. Two
/// nodes admitting at the same point in the same programme therefore differ in
/// air time by exactly their device delays, permanently, and waiting cannot
/// correct it because waiting only makes a node later. Running the ring at
/// different depths can, which is feasible only because a ring fills at decode
/// speed rather than in real time -- it drains in real time, it does not fill
/// that way.
///
/// So each node runs at `depth = Total - device_delay`, for one `Total` common
/// to the fleet, and `depth <= capacity` caps `Total` at
/// `capacity + min(device delay)` `[LOG-ECHO-030]`. That is a property of the
/// **fleet**, not a constraint on which node may be master: the node with the
/// smallest delay simply runs the fullest ring, and any node may announce
/// `[GDE-ECHO-315]`.
///
/// `TooShallow` therefore reports a `Total` this node cannot reach, which in
/// practice means the announcer computed it from its own full ring instead of
/// from the fleet's cap -- exactly what `schedule_for_admission` does today.
/// With `bose` at 46 ms and `vainopi` at 355 ms `[LOG-CPAL-060]`, a `vainopi`
/// announcing off a full ring asks for 15.355 s and `bose` can reach 15.046 s;
/// the fix is the announced total, not the choice of master.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    /// Admit with exactly this many frames ahead of sample 0 in the ring.
    Depth(u64),
    /// This node cannot be late enough: the master's submit-to-air distance
    /// exceeds this node's ring plus its device delay. Reported rather than
    /// clamped, because a clamped depth plays *early* on every passage and
    /// looks like a working fleet with a drift problem.
    TooShallow { short_by_frames: u64 },
    /// The instant has passed -- the schedule arrived later than this node's
    /// own device delay leaves room for.
    Late { by: Duration },
}

pub fn placement(
    sched: &Schedule,
    node: NodeTiming,
    ring_capacity_frames: u64,
    now: WallNanos,
) -> Placement {
    let rate = node.rate.max(1) as u64;
    let Some(ahead_ns) = sched.sound_at.checked_sub(now) else {
        return Placement::Late { by: Duration::from_nanos(now - sched.sound_at) };
    };
    // Round to nearest, not down. `sound_at` was itself built from a frame
    // count divided into nanoseconds, so truncating here loses whatever that
    // division dropped -- and it loses it in one direction, making every node
    // a frame or two shallow and therefore early. 23 us is inaudible; a
    // systematic sign is still worth not having `[GOV-SRC-040]`.
    let ahead_frames = (ahead_ns.saturating_mul(rate) + 500_000_000) / 1_000_000_000;
    let Some(depth) = ahead_frames.checked_sub(node.presentation_offset_frames) else {
        // The sound is nearer than this node's device delay: even a depth of
        // zero is too late.
        let short = node.presentation_offset_frames - ahead_frames;
        return Placement::Late { by: frames_to_duration(short, node.rate) };
    };
    if depth > ring_capacity_frames {
        return Placement::TooShallow { short_by_frames: depth - ring_capacity_frames };
    }
    Placement::Depth(depth)
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
    pub fn anchor(&self, rate: u32, ppm: Option<f64>) -> DriftAnchor {
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
    start_sample: u64,
    ring_frames: u64,
    device_delay_frames: u64,
    rate: u32,
    now: WallNanos,
) -> Schedule {
    let ahead_ns = (ring_frames + device_delay_frames) * 1_000_000_000 / rate.max(1) as u64;
    Schedule { passage_id, start_sample, sound_at: now + ahead_ns, rate }
}

/// What a master publishes for echo nodes, on the snapshot's own cadence.
///
/// `[GDE-ECHO-310]` puts both messages on the WebSocket the browser snapshot
/// already uses, which is why the transport is uninteresting: it exists.
///
/// `voided_by` is carried rather than implied by a missing anchor. A node that
/// receives nothing cannot tell "the master is quiet" from "the master cannot
/// currently place itself", and those call for different behaviour -- the
/// second means hold position and wait for the next passage boundary
/// `[GDE-ECHO-360]`, not go independent `[GDE-ECHO-500]`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EchoState {
    pub anchor: Option<DriftAnchor>,
    pub schedule: Option<Schedule>,
    pub voided_by: Option<Voided>,
}

/// Everything that voids the frame clock as a basis for an anchor.
///
/// `[GDE-ECHO-360]`. Each of these must force a rejoin at the next passage
/// boundary rather than a silent continuation on stale state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// What an echo node decides to do about the master's forward schedule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follow {
    /// Nothing new to act on -- no schedule, or one already acted on.
    Idle,
    /// Begin this passage by submitting sample 0 at this instant.
    StartAt { passage_id: i64, start_sample: u64, at: WallNanos },
    /// The schedule cannot be honoured: submission was already due.
    ///
    /// Distinct from `Idle` because it is not nothing happening. A node with a
    /// large presentation offset misses schedules a low-offset node makes
    /// comfortably `[GDE-ECHO-410]`, and it must rejoin at the next boundary
    /// rather than start late and be trimmed towards a master it never caught.
    Missed { passage_id: i64 },
    /// The master says it cannot currently place itself in time.
    ///
    /// **Hold, do not go independent.** `[GDE-ECHO-500]`'s handover is for a
    /// master that has gone *away*; this one is present and honest, and will
    /// re-establish at its own next passage boundary `[GDE-ECHO-360]`.
    Hold(Voided),
}

/// An echo node's side of the wire.
///
/// Holds no transport and no queue: it decides, and something else acts. That
/// keeps every branch here reachable from a test, which `[GDE-ECHO-370]`
/// asks for precisely because a sign error in this arithmetic is inaudible
/// until hours have passed.
#[derive(Clone, Debug)]
pub struct Follower {
    pub timing: NodeTiming,
    /// This node's own basis -- its own underruns and reopens, not the
    /// master's. Both must be sound before a residual means anything.
    pub basis: Basis,
    pub deadband: Duration,
    pub min_trim_interval: Duration,
    /// The last schedule acted on, whole.
    ///
    /// Keyed on the entire message rather than its `passage_id`, because a
    /// seek re-announces the passage already playing `[GDE-ECHO-325]` -- and a
    /// dedup on the id alone would discard exactly the message the seek exists
    /// to deliver. Comparing the whole thing is also the natural reading of
    /// `[GDE-ECHO-320]`: every message is absolute, so two identical ones are
    /// the same instruction and any difference is a new one.
    acted: Option<Schedule>,
    last_trim: Option<WallNanos>,
}

impl Follower {
    pub fn new(timing: NodeTiming, deadband: Duration, min_trim_interval: Duration) -> Self {
        Self { timing, basis: Basis::default(), deadband, min_trim_interval,
               acted: None, last_trim: None }
    }

    /// What to do about the master's schedule, if anything.
    ///
    /// Idempotent by passage: the same schedule arriving twice a second for
    /// fifteen seconds produces one `StartAt` and then `Idle`, which is what
    /// lets `[GDE-ECHO-320]` repeat every message without a sequence number.
    pub fn on_state(&mut self, st: &EchoState, now: WallNanos) -> Follow {
        if let Some(why) = st.voided_by {
            return Follow::Hold(why);
        }
        let Some(sched) = st.schedule else { return Follow::Idle };
        if self.acted == Some(sched) {
            return Follow::Idle;
        }
        self.acted = Some(sched);
        match submit_at(&sched, self.timing, now) {
            Some(at) => Follow::StartAt {
                passage_id: sched.passage_id, start_sample: sched.start_sample, at },
            None => Follow::Missed { passage_id: sched.passage_id },
        }
    }

    /// Whether to trim, given the master's anchor and this node's own air.
    ///
    /// Returns `None` when no comparison is possible at all -- a missing
    /// anchor, a voided basis on either side, or the two describing *different
    /// passages*, which is the case a naive implementation would silently
    /// treat as an enormous error and trim hard against `[GOV-SRC-040]`.
    pub fn trim_for(
        &mut self,
        st: &EchoState,
        local: Option<&AirPosition>,
        now: WallNanos,
    ) -> Option<Trim> {
        if st.voided_by.is_some() {
            return None;
        }
        let anchor = st.anchor?;
        let local = local?;
        if local.passage_id != anchor.passage_id {
            return None;
        }
        // Where the master says this node's own sample should have been heard,
        // and where it actually was. Both are on the disciplined wall clock,
        // so the difference is a real offset rather than a clock comparison.
        let local_heard = local.at;
        let residual = residual_ns(&anchor, local_heard);
        let since = self.last_trim.map_or(self.min_trim_interval, |t| {
            Duration::from_nanos(now.saturating_sub(t))
        });
        let t = trim_decision(&self.basis, residual, self.deadband, since,
                              self.min_trim_interval);
        if t != Trim::None {
            self.last_trim = Some(now);
        }
        Some(t)
    }
}

/// Where a queue entry came from, which is the whole of what a rejoin needs
/// to know about it `[GDE-ECHO-510]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Announced by the master.
    Announced,
    /// Chosen locally to keep the queue full while no announcement had
    /// arrived. Discarded the moment the master is heard from again -- it was
    /// only ever filling a gap.
    Local,
}

/// What the queue should become, and what the local Director still owes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueuePlan {
    /// The queue in play order, after reconciliation.
    pub queue: Vec<(i64, Origin)>,
    /// Entries the warm Director must supply to reach the configured depth.
    ///
    /// `[GDE-ECHO-500]`'s hysteresis: the queue is **topped up** rather than
    /// allowed to drain, so there is no moment of decision and no threshold to
    /// tune. Announced entries leave from the front while local ones fill in
    /// behind, and the changeover is a blend rather than an event.
    pub want_local: usize,
    /// Locally-chosen entries dropped because the master was heard from.
    pub discarded_local: usize,
}

/// Reconcile the local queue against what the master has announced.
///
/// **Announcements win outright.** `[GDE-ECHO-510]` is explicit that when
/// contact returns the master's queue takes precedence immediately and
/// locally-chosen entries still waiting are discarded. Discarding entries that
/// are about to be re-chosen looks wasteful and is the specified behaviour:
/// they were gap-fillers, and a node rejoining the fleet should be playing the
/// fleet's programme rather than a blend of two.
///
/// With no announcement at all this keeps what is there and reports the
/// shortfall, which is the going-independent path `[GDE-ECHO-500]` -- and it
/// is the same code, not a mode.
pub fn reconcile_queue(local: &[(i64, Origin)], announced: &[i64], depth: usize) -> QueuePlan {
    if announced.is_empty() {
        let kept: Vec<_> = local.to_vec();
        let want = depth.saturating_sub(kept.len());
        return QueuePlan { queue: kept, want_local: want, discarded_local: 0 };
    }
    let discarded = local.iter().filter(|(_, o)| *o == Origin::Local).count();
    let queue: Vec<_> = announced.iter().map(|id| (*id, Origin::Announced)).collect();
    let want = depth.saturating_sub(queue.len());
    QueuePlan { queue, want_local: want, discarded_local: discarded }
}

/// What to do with what is sounding when the master's programme returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejoin {
    /// Let the current passage finish. The master's programme is taken up at
    /// its natural end, or at a Skip `[REQ-AUD-162]`.
    ///
    /// Cutting a passage short to rejoin would make reconnection audible for
    /// no benefit: the node was never playing anything *wrong*, only something
    /// different `[GDE-ECHO-510]`.
    PlayOut,
    /// Nothing is sounding, so join the master's passage **mid-passage**, at
    /// the offset it has already reached.
    ///
    /// This is the capability `[GDE-ECHO-330]` deferred; the rejoin case
    /// promotes it from optional to required, because by the time a node
    /// rejoins the master is always part-way through something.
    JoinMidPassage { passage_id: i64, at_ms: u64 },
}

/// Decide between playing out and joining, given what is sounding here and
/// where the master is.
pub fn rejoin_action(sounding: Option<i64>, master: Option<&AirPosition>) -> Option<Rejoin> {
    let m = master?;
    match sounding {
        // Already on the master's passage: nothing to rejoin to.
        Some(id) if id == m.passage_id => None,
        Some(_) => Some(Rejoin::PlayOut),
        None => Some(Rejoin::JoinMidPassage { passage_id: m.passage_id, at_ms: m.position_ms }),
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
        let sched = Schedule { passage_id: 7, start_sample: 0, sound_at: 100 * SEC, rate: 44100 };
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
        let sched = Schedule { passage_id: 1, start_sample: 0, sound_at: 50 * SEC, rate: 44100 };
        let submit = submit_at(&sched, BOSE, 0).unwrap();
        assert_eq!(submit, 50 * SEC - BOSE.offset().as_nanos() as u64);
    }

    #[test]
    fn a_schedule_already_past_is_refused_not_rounded_forward() {
        // 100 ms of lead against a 355 ms offset: submission was due 255 ms
        // ago. Returning a past instant would read like an instruction.
        let sched = Schedule { passage_id: 2, start_sample: 0, sound_at: 10 * SEC + 100_000_000, rate: 44100 };
        assert!(submit_at(&sched, VAINOPI, 10 * SEC).is_none());
        // The same schedule is comfortably reachable by the low-offset node.
        assert!(submit_at(&sched, BOSE, 10 * SEC).is_some());
    }

    #[test]
    fn fifteen_seconds_of_lead_clears_every_measured_offset() {
        // `[GDE-ECHO-310]`'s claim, checked rather than asserted.
        let now = 1000 * SEC;
        let sched = Schedule { passage_id: 3, start_sample: 0, sound_at: now + 15 * SEC, rate: 44100 };
        for node in [BOSE, VAINOPI] {
            assert!(submit_at(&sched, node, now).is_some());
        }
    }

    #[test]
    fn residual_sign_says_late_is_positive() {
        let a = DriftAnchor {
            passage_id: 4, sample: 44100, heard_at: 5 * SEC, rate: 44100, ppm: None,
        };
        assert!(residual_ns(&a, 5 * SEC + 1_000_000) > 0, "later than master is positive");
        assert!(residual_ns(&a, 5 * SEC - 1_000_000) < 0, "earlier than master is negative");
        assert_eq!(residual_ns(&a, 5 * SEC), 0);
    }

    // `[REQ-AUD-160]`'s ring is ~15 s at 44100.
    const RING: u64 = 44100 * 15;

    /// The trap the type exists to stop: admitting at `submit_at` would put
    /// sample 0 a full ring late. Depth, not admission time, is the knob.
    #[test]
    fn placement_is_a_depth_not_an_admission_time() {
        let now = 100 * SEC;
        // A master with `bose`'s pipeline: 15.0 s of ring plus 46 ms of device.
        let s = schedule_for_admission(7, 0, RING, BOSE.presentation_offset_frames, 44100, now);
        // The follower is the same node shape, so it runs a full ring.
        assert_eq!(placement(&s, BOSE, RING, now), Placement::Depth(RING));
        // And `submit_at` is a *device* instant, one device delay before the
        // sound -- not the moment to admit. The two differ by the whole ring.
        let submit = submit_at(&s, BOSE, now).unwrap();
        assert_eq!(submit - now, frames_to_duration(RING, 44100).as_nanos() as u64);
    }

    /// `[LOG-ECHO-030]`: a larger device delay is absorbed by running shallower.
    #[test]
    fn a_slower_device_runs_a_shallower_ring() {
        let now = 100 * SEC;
        let s = schedule_for_admission(7, 0, RING, BOSE.presentation_offset_frames, 44100, now);
        // vainopi's 355 ms of A2DP comes out of its ring, exactly.
        let want = RING + BOSE.presentation_offset_frames - VAINOPI.presentation_offset_frames;
        assert_eq!(placement(&s, VAINOPI, RING, now), Placement::Depth(want));
        assert_eq!(RING - want, 13633, "vainopi runs 309 ms shallower than bose");
    }

    /// A total computed off a full ring by the *slower* node is unreachable for
    /// the faster one -- and must say so rather than clamp and play early
    /// forever. This is a defect in the announced total, not in who announces
    /// `[GDE-ECHO-315]`; the same pair works at a total the fleet can meet, as
    /// the next test shows.
    #[test]
    fn a_total_announced_off_a_full_ring_can_be_unreachable() {
        let now = 100 * SEC;
        let s = schedule_for_admission(7, 0, RING, VAINOPI.presentation_offset_frames, 44100, now);
        match placement(&s, BOSE, RING, now) {
            Placement::TooShallow { short_by_frames } => {
                assert_eq!(short_by_frames,
                    VAINOPI.presentation_offset_frames - BOSE.presentation_offset_frames);
            }
            other => panic!("expected TooShallow, got {other:?}"),
        }
    }

    /// `[GDE-ECHO-315]`: the slower node announcing is fine, and is in fact the
    /// preferred arrangement, so long as it announces the fleet's total rather
    /// than its own full ring. `vainopi` then runs the shallow ring it would
    /// have run anyway, and `bose` runs full -- the same two depths as when
    /// `bose` announces. Who announces does not enter the arithmetic.
    #[test]
    fn the_slower_node_may_announce_at_the_fleets_total() {
        let now = 100 * SEC;
        // The fleet's cap: capacity + the SMALLEST device delay. vainopi holds
        // the larger, so it announces off a ring short by the difference.
        let vainopi_depth = RING + BOSE.presentation_offset_frames
            - VAINOPI.presentation_offset_frames;
        let s = schedule_for_admission(
            7, 0, vainopi_depth, VAINOPI.presentation_offset_frames, 44100, now);
        assert_eq!(placement(&s, BOSE, RING, now), Placement::Depth(RING),
            "the smallest-delay node runs the fullest ring, whoever announced");
        assert_eq!(placement(&s, VAINOPI, RING, now), Placement::Depth(vainopi_depth));
    }

    /// Network lateness spends the ring, and there is ~15 s of it to spend --
    /// which is the margin `[GDE-ECHO-310]` claims, correctly, once it is
    /// measured against latency rather than against the offset.
    #[test]
    fn lateness_spends_depth_and_there_is_plenty() {
        let emitted = 100 * SEC;
        let s = schedule_for_admission(7, 0, RING, BOSE.presentation_offset_frames, 44100, emitted);
        // Five seconds late: still fine, just a shallower ring.
        assert_eq!(placement(&s, BOSE, RING, emitted + 5 * SEC), Placement::Depth(RING - 44100 * 5));
        // Past the sound itself: named, not clamped.
        assert!(matches!(placement(&s, BOSE, RING, emitted + 20 * SEC), Placement::Late { .. }));
    }

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
        let s = schedule_for_admission(5, 0, RING, BOSE.presentation_offset_frames, 44100, now);
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
        let anchor = a.anchor(44100, Some(-2.09));
        assert_eq!(anchor.sample, 441_000);           // 10 s at 44100
        assert_eq!(anchor.heard_at, 12 * SEC);
        assert_eq!(residual_ns(&anchor, 12 * SEC), 0);
    }

    #[test]
    fn two_nodes_agreeing_on_the_air_have_no_residual() {
        // The property echo is trying to hold: different offsets, same sound.
        let sched = Schedule { passage_id: 8, start_sample: 0, sound_at: 900 * SEC, rate: 44100 };
        let now = 880 * SEC;
        let b = submit_at(&sched, BOSE, now).unwrap() + BOSE.offset().as_nanos() as u64;
        let v = submit_at(&sched, VAINOPI, now).unwrap() + VAINOPI.offset().as_nanos() as u64;
        assert_eq!(b, v, "submit + own offset must land on the same instant");
        assert_eq!(b, sched.sound_at);
    }

    fn follower(t: NodeTiming) -> Follower {
        Follower::new(t, Duration::from_micros(500), Duration::from_secs(1))
    }

    fn state_with(sched: Option<Schedule>, anchor: Option<DriftAnchor>) -> EchoState {
        EchoState { anchor, schedule: sched, voided_by: None }
    }

    /// The case the dedup key was changed for `[GDE-ECHO-325]`. A seek
    /// re-announces the passage already playing, so a follower keyed on
    /// `passage_id` alone would call it Idle and never move -- swallowing
    /// precisely the message the seek exists to deliver.
    #[test]
    fn a_seek_re_announces_the_playing_passage_and_is_acted_on() {
        let mut f = follower(BOSE);
        let playing = Schedule { passage_id: 4, start_sample: 0, sound_at: 100 * SEC, rate: 44100 };
        assert!(matches!(f.on_state(&state_with(Some(playing), None), 80 * SEC),
                         Follow::StartAt { passage_id: 4, start_sample: 0, .. }));
        assert_eq!(f.on_state(&state_with(Some(playing), None), 80 * SEC), Follow::Idle);

        // Same passage, three minutes in, sounding much sooner.
        let seek = Schedule { passage_id: 4, start_sample: 180 * 44100,
                              sound_at: 81 * SEC, rate: 44100 };
        match f.on_state(&state_with(Some(seek), None), 80 * SEC) {
            Follow::StartAt { passage_id: 4, start_sample, .. } => {
                assert_eq!(start_sample, 180 * 44100, "the offset must survive the wire");
            }
            other => panic!("a seek was not acted on: {other:?}"),
        }
        // And is itself idempotent afterwards.
        assert_eq!(f.on_state(&state_with(Some(seek), None), 80 * SEC), Follow::Idle);
    }

    /// Two seeks to the same point at different times are different
    /// instructions, and the second must not be mistaken for a repeat.
    #[test]
    fn the_same_offset_announced_again_later_is_a_new_instruction() {
        let mut f = follower(BOSE);
        let a = Schedule { passage_id: 9, start_sample: 44100, sound_at: 90 * SEC, rate: 44100 };
        let b = Schedule { passage_id: 9, start_sample: 44100, sound_at: 95 * SEC, rate: 44100 };
        assert!(matches!(f.on_state(&state_with(Some(a), None), 80 * SEC), Follow::StartAt { .. }));
        assert!(matches!(f.on_state(&state_with(Some(b), None), 80 * SEC), Follow::StartAt { .. }));
    }

    #[test]
    fn a_repeated_schedule_is_acted_on_once() {
        // `[GDE-ECHO-320]` repeats every message rather than sequencing them,
        // so the follower has to be the thing that makes it idempotent.
        let sched = Schedule { passage_id: 4, start_sample: 0, sound_at: 100 * SEC, rate: 44100 };
        let st = state_with(Some(sched), None);
        let mut f = follower(BOSE);
        assert!(matches!(f.on_state(&st, 80 * SEC), Follow::StartAt { passage_id: 4, .. }));
        for _ in 0..30 {
            assert_eq!(f.on_state(&st, 80 * SEC), Follow::Idle);
        }
    }

    #[test]
    fn a_high_offset_node_can_miss_what_a_low_offset_node_makes() {
        // 100 ms of lead: bose can still submit, vainopi needed to 255 ms ago.
        let sched = Schedule { passage_id: 5, start_sample: 0, sound_at: 10 * SEC + 100_000_000, rate: 44100 };
        let st = state_with(Some(sched), None);
        assert!(matches!(follower(BOSE).on_state(&st, 10 * SEC),
                         Follow::StartAt { .. }));
        assert_eq!(follower(VAINOPI).on_state(&st, 10 * SEC),
                   Follow::Missed { passage_id: 5 });
    }

    #[test]
    fn a_master_that_cannot_place_itself_is_held_not_abandoned() {
        // `[GDE-ECHO-500]`'s handover is for a master that has gone away. This
        // one is present and saying so, and will recover at its own next
        // boundary -- going independent here would desynchronise on purpose.
        let st = EchoState {
            anchor: None,
            schedule: Some(Schedule { passage_id: 6, start_sample: 0, sound_at: 100 * SEC, rate: 44100 }),
            voided_by: Some(Voided::Underrun),
        };
        let mut f = follower(BOSE);
        assert_eq!(f.on_state(&st, 80 * SEC), Follow::Hold(Voided::Underrun));
        assert_eq!(f.trim_for(&st, None, 80 * SEC), None, "and no trim while held");
    }

    #[test]
    fn anchors_for_a_different_passage_are_not_a_huge_error() {
        // The trap: the master is on passage 7 and this node still on 6, so
        // the positions differ by minutes. Trimming against that would drive
        // the node hard in the wrong direction.
        let anchor = DriftAnchor {
            passage_id: 7, sample: 0, heard_at: 500 * SEC, rate: 44100, ppm: None,
        };
        let local = AirPosition { passage_id: 6, position_ms: 0, at: 200 * SEC };
        let st = state_with(None, Some(anchor));
        let mut f = follower(BOSE);
        f.basis.establish();
        assert_eq!(f.trim_for(&st, Some(&local), 500 * SEC), None);
    }

    #[test]
    fn a_node_with_no_basis_of_its_own_does_not_trim() {
        let anchor = DriftAnchor {
            passage_id: 1, sample: 0, heard_at: 100 * SEC, rate: 44100, ppm: None,
        };
        let local = AirPosition { passage_id: 1, position_ms: 0, at: 100 * SEC + 5_000_000 };
        let st = state_with(None, Some(anchor));
        let mut f = follower(BOSE);           // basis never established
        assert_eq!(f.trim_for(&st, Some(&local), 100 * SEC), Some(Trim::None));
    }

    #[test]
    fn a_late_follower_drops_and_then_waits_out_the_interval() {
        let anchor = DriftAnchor {
            passage_id: 1, sample: 0, heard_at: 100 * SEC, rate: 44100, ppm: None,
        };
        // 5 ms late.
        let local = AirPosition { passage_id: 1, position_ms: 0, at: 100 * SEC + 5_000_000 };
        let st = state_with(None, Some(anchor));
        let mut f = follower(BOSE);
        f.basis.establish();
        assert_eq!(f.trim_for(&st, Some(&local), 100 * SEC), Some(Trim::DropFrame));
        // Immediately after, the rate limit holds even though still 5 ms out.
        assert_eq!(f.trim_for(&st, Some(&local), 100 * SEC), Some(Trim::None));
        // A second later it may act again.
        assert_eq!(f.trim_for(&st, Some(&local), 101 * SEC), Some(Trim::DropFrame));
    }

    #[test]
    fn an_early_follower_duplicates() {
        let anchor = DriftAnchor {
            passage_id: 1, sample: 0, heard_at: 100 * SEC, rate: 44100, ppm: None,
        };
        let local = AirPosition { passage_id: 1, position_ms: 0, at: 100 * SEC - 5_000_000 };
        let st = state_with(None, Some(anchor));
        let mut f = follower(VAINOPI);
        f.basis.establish();
        assert_eq!(f.trim_for(&st, Some(&local), 100 * SEC), Some(Trim::DuplicateFrame));
    }

    use Origin::{Announced, Local};

    #[test]
    fn silence_tops_the_queue_up_rather_than_letting_it_drain() {
        // `[GDE-ECHO-500]`: no timeout, no threshold. The queue simply never
        // runs dry, so there is no moment of decision.
        let local = vec![(1, Announced), (2, Announced)];
        let p = reconcile_queue(&local, &[], 5);
        assert_eq!(p.queue, local, "nothing announced, nothing replaced");
        assert_eq!(p.want_local, 3, "the Director owes three to reach depth");
        assert_eq!(p.discarded_local, 0);
    }

    #[test]
    fn a_node_out_of_contact_ends_up_entirely_local_without_a_cliff() {
        // Five announced passages is roughly twenty minutes of runway. Drain
        // them one at a time and the queue refills locally behind -- the
        // changeover is a blend rather than an event.
        let mut q: Vec<(i64, Origin)> = (1..=5).map(|i| (i, Announced)).collect();
        for step in 0..5 {
            q.remove(0); // a passage completes
            let p = reconcile_queue(&q, &[], 5);
            assert_eq!(p.want_local, 1, "exactly one selection per completion");
            q = p.queue;
            q.push((100 + step, Local));
            assert_eq!(q.len(), 5, "never below depth");
        }
        assert!(q.iter().all(|(_, o)| *o == Local), "fully independent, gradually");
    }

    #[test]
    fn announcements_win_and_gap_fillers_are_discarded() {
        // `[GDE-ECHO-510]`: the master's queue takes precedence immediately.
        let local = vec![(90, Announced), (101, Local), (102, Local)];
        let p = reconcile_queue(&local, &[7, 8, 9, 10, 11], 5);
        assert_eq!(p.queue.len(), 5);
        assert!(p.queue.iter().all(|(_, o)| *o == Announced));
        assert_eq!(p.queue[0].0, 7, "in the master's order");
        assert_eq!(p.discarded_local, 2, "both gap-fillers dropped");
        assert_eq!(p.want_local, 0);
    }

    #[test]
    fn a_short_announcement_is_topped_up_behind_rather_than_padded_with_locals() {
        // Discarding entries that are about to be re-chosen looks wasteful and
        // is the specified behaviour: a rejoining node plays the fleet's
        // programme, not a blend of two.
        let local = vec![(101, Local), (102, Local)];
        let p = reconcile_queue(&local, &[7, 8], 5);
        assert_eq!(p.queue, vec![(7, Announced), (8, Announced)]);
        assert_eq!(p.discarded_local, 2);
        assert_eq!(p.want_local, 3, "fresh selections, behind the announced");
    }

    #[test]
    fn rejoining_never_cuts_the_passage_in_progress_short() {
        // The node was never playing anything wrong, only something different.
        let master = AirPosition { passage_id: 42, position_ms: 95_000, at: 0 };
        assert_eq!(rejoin_action(Some(7), Some(&master)), Some(Rejoin::PlayOut));
    }

    #[test]
    fn with_nothing_sounding_it_joins_mid_passage_not_at_the_start() {
        // `[GDE-ECHO-330]` deferred mid-passage joining; the rejoin case makes
        // it required, because the master is always part-way through by then.
        let master = AirPosition { passage_id: 42, position_ms: 95_000, at: 0 };
        assert_eq!(rejoin_action(None, Some(&master)),
                   Some(Rejoin::JoinMidPassage { passage_id: 42, at_ms: 95_000 }));
    }

    #[test]
    fn a_node_already_on_the_masters_passage_has_nothing_to_rejoin() {
        let master = AirPosition { passage_id: 42, position_ms: 95_000, at: 0 };
        assert_eq!(rejoin_action(Some(42), Some(&master)), None);
        assert_eq!(rejoin_action(Some(7), None), None, "and no master, no action");
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
