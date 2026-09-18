//! Following a master, from inside the player `[GDE-ECHO-330]`.
//!
//! `echoprobe` proved the wire can be read; this is the same reading wired to
//! an engine that can act on it. The division of labour is deliberate and
//! narrow: everything about *when* and *how far in* is decided by
//! [`crate::echo`], everything about *what plays* is decided by the engine,
//! and this module only carries messages between them and resolves a passage
//! id against the local library.
//!
//! It resolves ids because an echo node holds the same music on its own disk
//! `[GDE-ECHO-420]` -- that is the capability the whole design rests on. What
//! crosses the network is an intention, never audio.
//!
//! **A master is not trusted with this node's timing.** The schedule says when
//! a sample should sound; this node's own presentation offset decides when to
//! submit for that to happen `[GDE-ECHO-410]`, and a schedule it cannot meet
//! is declined rather than approximated.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;

use crate::echo::{EchoState, Follow, Follower, NodeTiming};
use crate::engine::{Command, EngineHandle};

/// Only the field an echo node cares about. Serde ignores the rest of the
/// snapshot, which is how this rides a socket built for a browser.
#[derive(serde::Deserialize)]
struct Snapshot {
    #[serde(default)]
    echo: EchoState,
    /// What the node being followed will play next, in order `[GDE-ECHO-500]`.
    ///
    /// Already on this socket for the browser's sake, and ignored until now,
    /// which is why a follower's own "coming up" disagreed with the master's:
    /// it kept choosing for itself and was overridden one passage at a time.
    #[serde(default)]
    queue: Vec<AnnouncedEntry>,
}

#[derive(serde::Deserialize)]
struct AnnouncedEntry {
    passage_id: i64,
}

pub struct Following {
    pub timing: NodeTiming,
    pub db: PathBuf,
    pub library: PathBuf,
}

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Follow `cfg.url` until the process ends, reconnecting as needed.
///
/// Reconnection is a plain retry with no backoff state and no resynchronising
/// handshake, because there is nothing to resynchronise: every message is
/// absolute and the next one to arrive is sufficient on its own
/// `[GDE-ECHO-320]`. A node that misses an hour of them rejoins on the first
/// one it sees.
/// Read the host the listener has set, as a URL, or `None` for independent.
///
/// A bare name gets no port, which is port 80 -- what `bose` serves. A node on
/// another port is named with one, `lempiplay3:5720`, because the fleet is not
/// uniform and pretending otherwise would make the control work on some nodes
/// and silently not on others `[SPEC-ECHO-010]`.
fn wanted_url(handle: &EngineHandle) -> Option<String> {
    let host = handle.state.lock().ok()?.echo_node.follow_host.clone();
    let host = host.trim();
    (!host.is_empty()).then(|| format!("ws://{host}/ws"))
}

/// How far ahead of itself a mid-passage join aims `[SPEC-ECHO-030]`.
///
/// It has to cover a 10 ms tick, the engine opening and first-decoding the
/// file -- tens of milliseconds on a Pi -- and whatever the snapshot took to
/// arrive. A second is comfortably more than all of that and comfortably
/// inside `[GDE-ECHO-325]`'s five-second allowance for a resync. Aiming too
/// close simply fails the join and waits for the next snapshot, which is why
/// this errs long.
const MID_JOIN_MARGIN: Duration = Duration::from_secs(1);

/// How far out this node may be before the offset correction acts.
///
/// **Set by the measurement, not by the ear.** A listener can place two
/// speakers to a millisecond `[SPEC-DLY-020]`, but the anchor this residual
/// comes from carries the output ring's own depth jitter -- tens of
/// milliseconds `[LOG-P4-010]` -- so a deadband below that would have the node
/// chasing noise, which is the exact failure `[GDE-ECHO-350]` exists to
/// prevent. Alignment finer than this needs a more precise anchor, not a
/// smaller number here.
const OFFSET_DEADBAND: Duration = Duration::from_millis(8);

/// Below this the boundary knobs cannot help and the frame trim takes over
/// `[GDE-ECHO-349]`.
///
/// One mix quantum, 46 ms at 44.1 kHz stereo, rounded up. Above it a passage
/// boundary can shift the whole error at once; below it the coarse knob's
/// step is larger than the error itself and only the 23 us actuator will do.
const OFFSET_ENDGAME: Duration = Duration::from_millis(50);

/// How far this node's own transition may sit from the announced one and
/// still be called the same transition `[GDE-ECHO-353]`.
///
/// Generous, because it is separating two cases that are far apart rather
/// than measuring either: an ordinary boundary both nodes are heading to
/// within a second or two of each other, against a skip that moved the
/// master's by minutes. The offset correction handles everything inside it
/// `[GDE-ECHO-340]`, and a scheduled start would be the wrong tool there
/// anyway -- it cuts the ring.
const FLOW_TOLERANCE: Duration = Duration::from_secs(5);

/// How long a committed placement suppresses the other way of placing
/// `[GDE-ECHO-354]`.
///
/// Long enough to cover a scheduled start's lead and the engine acting on it;
/// short enough that a start the engine declined is retried rather than
/// leaving the node stranded on the wrong passage `[GDE-ECHO-355]`.
const COMMITMENT_HOLDS: Duration = Duration::from_secs(6);

/// How the filtered residual is taken `[GDE-ECHO-348]`.
const RESIDUAL_WINDOW: Duration = Duration::from_secs(120);
const RESIDUAL_MIN_SAMPLES: usize = 60;

/// The most a single transition may be asked to absorb.
///
/// Half a second of a crossfade made longer or shorter is not something a
/// listener can point at, and a larger offset is simply taken in more than one
/// bite `[GDE-ECHO-341]`.
const OFFSET_MAX_BITE: Duration = Duration::from_millis(500);

/// Beyond this, nudging is too slow to be the whole answer and the node
/// places its first sample afresh instead `[GDE-ECHO-344]`.
///
/// **Above the join bias, and deliberately.** A join lands a few hundred
/// milliseconds to a second late `[GDE-ECHO-342]`; a threshold at or below
/// that would have every join trigger the next one, for ever. 1.5 s clears the
/// worst bias seen with room to spare, so a join always lands *inside* the
/// band and the nudges take it from there.
///
/// It also has to be low enough to matter: at 500 ms a transition and four
/// minutes a passage, an offset of five seconds takes forty minutes to nudge
/// away, which is not convergence a listener would recognise as such.
const OFFSET_REJOIN_BEYOND: Duration = Duration::from_millis(1_500);

/// The rate fit's window, and what it takes before it means anything.
///
/// An hour, because 30 ms of anchor scatter sampled twice a second resolves a
/// slope to about 0.35 ppm over that span and only 5 ppm over ten minutes
/// `[RateEstimate]`. Fifteen minutes is the earliest anything is published,
/// and it is worth a couple of ppm then -- enough to start correcting the bulk
/// of a 14 ppm drift while the estimate sharpens.
const RATE_WINDOW: Duration = Duration::from_secs(3600);
const RATE_MIN_SPAN: Duration = Duration::from_secs(900);
const RATE_MIN_SAMPLES: usize = 200;

/// This node's presentation offset as it stands **now**.
///
/// Read every pass rather than taken at startup, because the listener can move
/// it from the settings panel at any moment `[SPEC-DLY-010]` and a follower
/// holding a figure from boot would schedule against a delay nobody has any
/// more. It is the measured half plus the calibrated one, already clamped by
/// the engine `[GDE-ECHO-430]`.
fn live_timing(handle: &EngineHandle, fallback: NodeTiming) -> NodeTiming {
    match handle.state.lock() {
        Ok(s) => NodeTiming {
            presentation_offset_frames: s.echo_node.offset_frames,
            rate: if s.echo_node.rate > 0 { s.echo_node.rate } else { fallback.rate },
        },
        Err(_) => fallback,
    }
}

/// This node's own anchor, as it publishes it to anyone following *it*.
fn own_anchor(handle: &EngineHandle) -> Option<crate::echo::DriftAnchor> {
    handle.state.lock().ok()?.echo.anchor
}

/// What one connection to a master accumulates.
///
/// Held together because every field is reset by the same event -- losing the
/// connection -- and because passing five of them separately is how a
/// parameter list stops being readable.
struct FollowState {
    /// The last line printed, so a steady state is not restated twice a second.
    note: String,
    /// The master passage a placement was last committed to, and when.
    ///
    /// **Expiring, not permanent.** It exists so the scheduled start and the
    /// mid-passage join stop racing each other `[GDE-ECHO-354]`, but a
    /// commitment the engine then declined -- too late to meet, say -- must
    /// not lock the passage out for ever. It did once: a skip the follower
    /// could not reach was declined, the marker blocked the join that would
    /// have rescued it, and the node played something else entirely
    /// `[GDE-ECHO-355]`.
    mid_joined: Option<(i64, std::time::Instant)>,
    /// The master passage an offset correction was last sent for.
    corrected: Option<i64>,
    /// The relative rate fit that drives the trim `[GDE-ECHO-340]`.
    rate: crate::echo::RateEstimate,
    /// The announced queue last adopted, so an unchanged one costs no
    /// database work. The snapshot arrives twice a second.
    queue: Vec<i64>,
    /// An offset too large for a transition to absorb, waiting for a
    /// scheduled start to place the first sample afresh `[GDE-ECHO-340]`.
    want_rejoin: bool,
    /// The followed node's clock, as this one estimates it `[GDE-ECHO-366]`.
    clock: crate::echo::MasterClock,
    /// The residual, seen through two minutes rather than one reading
    /// `[GDE-ECHO-348]`.
    filtered: crate::echo::ResidualFilter,
    /// The rate correction currently being applied, ppm `[GDE-ECHO-346]`.
    ///
    /// Carried because the fit measures what is LEFT after this correction,
    /// not the drift itself. Treating a fitted slope as the whole answer sets
    /// the trim to `R - A` when it already holds `A`, which settles at half
    /// the drift and oscillates about it.
    applied_ppm: f64,
}

impl FollowState {
    fn new() -> Self {
        Self {
            note: String::new(),
            mid_joined: None,
            corrected: None,
            rate: crate::echo::RateEstimate::new(
                RATE_WINDOW, RATE_MIN_SPAN, RATE_MIN_SAMPLES),
            queue: Vec::new(),
            want_rejoin: false,
            clock: crate::echo::MasterClock::new(
                CLOCK_WINDOW, Duration::from_secs(1)),
            applied_ppm: 0.0,
            filtered: crate::echo::ResidualFilter::new(
                RESIDUAL_WINDOW, RESIDUAL_MIN_SAMPLES),
        }
    }
}

/// How far two nodes' clocks may differ before nothing here means anything.
///
/// Generous, because it is not measuring quality: a healthy pair agrees to
/// milliseconds and the failure it catches is *days*, a node booted without an
/// RTC before NTP has stepped it `[GDE-ECHO-365]`. Thirty seconds is far past
/// any transport delay and far short of the fault.
const MAX_CLOCK_SKEW: Duration = Duration::from_secs(30);

/// How long a run of clock readings the offset is taken from.
///
/// Long enough that one quiet moment on the network supplies an
/// uncontaminated sample `[MasterClock]`, short enough to follow a master
/// whose own clock is being disciplined underneath it.
const CLOCK_WINDOW: Duration = Duration::from_secs(60);

/// The passage this node will play next of its own accord.
///
/// `None` when nothing is queued, which is the case a scheduled start exists
/// for.
fn next_up(handle: &EngineHandle) -> Option<i64> {
    handle.state.lock().ok()?.queue.first().map(|e| e.passage_id)
}

/// How long until this node reaches its own next passage, in ms.
///
/// `None` when nothing is playing, or when the passage has no length to
/// measure against -- a live capture, say -- in which case a follower has no
/// business guessing and should act on what it was told.
fn own_transition_in_ms(handle: &EngineHandle) -> Option<u64> {
    let s = handle.state.lock().ok()?;
    let cur = s.current.as_ref()?;
    let dur = cur.duration_ms();
    (dur > 0).then(|| dur.saturating_sub(s.position_ms))
}

/// Whether this node is playing that passage or is going to.
///
/// **Playing is not enough to ask.** `current` is the *audible* passage, and
/// the output ring is some fifteen seconds deep `[LOG-ECHO-020]`, so for a
/// long window after a follower admits a passage it still names the previous
/// one. The master's anchor switches as soon as its own ring drains, which is
/// sooner -- so a guard that only asks what is playing sees a mismatch at
/// every ordinary transition and joins into a passage the node was already
/// flowing into. Each of those joins cuts the ring and re-imposes the join
/// bias `[GDE-ECHO-342]`, which is how `lempiplay3` held a steady 0.9 s of lag
/// through a correction loop that was working perfectly `[GDE-ECHO-343]`.
///
/// The queue is the rest of the answer: a passage already coming needs no
/// join, only patience.
fn coming_here(handle: &EngineHandle, passage_id: i64) -> bool {
    let Ok(s) = handle.state.lock() else { return false };
    s.current.as_ref().is_some_and(|e| e.passage_id == passage_id)
        || s.queue.iter().any(|e| e.passage_id == passage_id)
}

/// Join at once, and what this node is playing, as the panel has them.
fn join_intent(handle: &EngineHandle) -> (bool, Option<i64>) {
    match handle.state.lock() {
        Ok(s) => (s.echo_node.join_now, s.current.as_ref().map(|e| e.passage_id)),
        Err(_) => (false, None),
    }
}

/// Say what following is actually doing, where the panel can see it.
///
/// The engine rewrites `echo_node` twice a second and deliberately carries
/// this field across untouched, because the engine does not know it: only the
/// follower does `[SPEC-ECHO-020]`.
fn set_status(handle: &EngineHandle, status: &str) {
    if let Ok(mut s) = handle.state.lock() {
        s.echo_node.follow_status = status.to_string();
    }
}

/// Follow whatever node the settings name, for as long as they name one.
///
/// Reconnection is a plain retry with no backoff state and no resynchronising
/// handshake, because there is nothing to resynchronise: every message is
/// absolute and the next to arrive is sufficient on its own `[GDE-ECHO-320]`.
/// A node that misses an hour of them rejoins on the first one it sees.
pub async fn run(cfg: Following, handle: Arc<EngineHandle>) {
    let mut last_note = String::new();
    loop {
        let Some(url) = wanted_url(&handle) else {
            // Independent is not an error and not a wait for anything; it is
            // what every node does by default `[GDE-ECHO-500]`.
            set_status(&handle, "");
            // And an independent node trims nothing. Leaving a rate behind
            // would have it quietly correcting towards a master it is no
            // longer listening to.
            // The integrator resets with it: `FollowState` is rebuilt per
            // connection, so a reconnection never resumes from a stale trim.
            handle.send(Command::SetEchoRate(0.0));
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        let mut follower =
            Follower::new(cfg.timing, Duration::from_micros(500), Duration::from_secs(1));
        // Per connection: a node that reconnects should catch up again, since
        // whatever it was playing while disconnected is by then its own.
        let mut fs = FollowState::new();
        // A follower's basis would normally be established and voided by its
        // own audio path. Nothing here trims, so nothing consults it; it is
        // established once and left alone, and that is said out loud because a
        // node that DOES trim must not do this `[GDE-ECHO-360]`.
        follower.basis.establish();

        let ws = match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                set_status(&handle, &format!("Cannot reach {url}: {e}"));
                note(&mut last_note, format!("echo-follow: connect to {url} failed: {e}"));
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        set_status(&handle, &format!("Connected to {url}, waiting for a passage to start."));
        note(&mut last_note, format!("echo-follow: connected to {url}"));
        let (_, mut rx) = ws.split();

        loop {
            // Bounded, so a setting changed while the master is quiet is
            // noticed. An unbounded await here would hold a node to a master
            // it was told to stop following until that master next spoke.
            let msg = match tokio::time::timeout(Duration::from_secs(2), rx.next()).await {
                Err(_) => {
                    if wanted_url(&handle).as_deref() != Some(url.as_str()) { break }
                    continue;
                }
                Ok(None) => break,
                Ok(Some(m)) => m,
            };
            if wanted_url(&handle).as_deref() != Some(url.as_str()) {
                note(&mut last_note, "echo-follow: the node to follow changed".to_string());
                break;
            }
            let text = match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => t,
                Ok(_) => continue,
                Err(e) => {
                    note(&mut last_note, format!("echo-follow: socket error: {e}"));
                    break;
                }
            };
            let Ok(snap) = serde_json::from_str::<Snapshot>(&text) else { continue };
            adopt_queue(&snap, &cfg, &handle, &mut fs).await;
            act(&mut follower, &snap.echo, &cfg, &handle, &mut fs).await;
        }
        set_status(&handle, "Not connected.");
        // Same reasoning as going independent: no master, no trim.
        handle.send(Command::SetEchoRate(0.0));
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn act(
    f: &mut Follower,
    st: &EchoState,
    cfg: &Following,
    handle: &Arc<EngineHandle>,
    fs: &mut FollowState,
) {
    // The offset the listener has set, this instant. Assigned rather than
    // passed, because `submit_at` and the trim both read it off the follower.
    f.timing = live_timing(handle, cfg.timing);
    // Everything below works in the MASTER's frame of time `[GDE-ECHO-366]`.
    // This node's own wall clock is used only to measure intervals against,
    // never compared with the wire, so a node booted days behind -- which is
    // every node here after a power cut, none having an RTC -- schedules
    // exactly as a disciplined one would. A step on either side is a
    // discontinuity like any other and clears the rate window.
    if let Some(m) = st.anchor.as_ref() {
        if fs.clock.observe(m.heard_at, now_nanos()) {
            note(&mut fs.note, "echo-follow: a clock stepped; re-measuring".to_string());
            fs.rate.clear();
            handle.send(Command::SetEchoRate(0.0));
        }
        // Still worth saying, but as a diagnosis rather than a refusal: this
        // node now follows correctly with a wrong clock, and a listener
        // should still be told the clock is wrong `[GDE-ECHO-365]`.
        if !crate::echo::clocks_agree(m.heard_at, now_nanos(), MAX_CLOCK_SKEW) {
            let skew = (now_nanos() as i64 - m.heard_at as i64) / 1_000_000_000;
            note(&mut fs.note, format!(
                "echo-follow: this node's clock is {skew} s from that one's; following anyway, on its clock"));
        }
    }
    // Without a reading there is no shared frame and nothing can be scheduled.
    let Some(now_master) = fs.clock.now(now_nanos()) else { return };

    // Catching up to what the master is ALREADY playing, which the schedule
    // cannot do: a schedule describes a passage about to start, and the moment
    // somebody switches a speaker into follower mode is almost never one
    // `[SPEC-ECHO-030]`.
    //
    // Attempted once per master passage. A node whose library lacks it must
    // not retry twice a second forever, and when the master moves on the
    // ordinary schedule path takes over anyway.
    let (join_now, _) = join_intent(handle);
    if join_now {
        if let Some(a) = st.anchor.as_ref() {
            // Coming to it is enough to skip the join only while the node is
            // roughly in the right place. Grossly out -- a node that has just
            // restarted and resumed its own programme, say -- it is playing
            // the right passage at the wrong moment, and only placing the
            // first sample afresh fixes that `[GDE-ECHO-344]`.
            let near = !fs.want_rejoin;
            let committed = fs.mid_joined.is_some_and(|(id, when)| {
                id == a.passage_id && when.elapsed() < COMMITMENT_HOLDS
            });
            let already = (coming_here(handle, a.passage_id) && near) || committed;
            if !already {
                fs.mid_joined = Some((a.passage_id, std::time::Instant::now()));
                let air = crate::echo::AirPosition {
                    passage_id: a.passage_id,
                    position_ms: a.sample.saturating_mul(1000) / a.rate.max(1) as u64,
                    at: a.heard_at,
                };
                let lead = f.timing.offset() + MID_JOIN_MARGIN;
                fs.want_rejoin = false;
                if let Some(j) = crate::echo::join_mid_passage(&air, f.timing, now_master, lead) {
                    // Back into this node's own clock before anyone waits on it.
                    let at = fs.clock.to_local(j.submit_at).unwrap_or(j.submit_at);
                    start(j.passage_id, j.start_sample, at, cfg, handle, &mut fs.note,
                          "joining part-way into").await;
                }
            }
        }
    }

    // `[GDE-ECHO-340]`'s offset correction. Measured now, applied at the next
    // admission -- which is sound only because an offset is a position and does
    // not grow while nobody is looking, unlike the rate error trimming handles.
    //
    // Once per master passage: the correction is for the passage after this
    // one, and sending it twice a second would simply overwrite itself.
    if let (Some(m), Some(mine)) = (st.anchor.as_ref(), own_anchor(handle)) {
        if m.passage_id == mine.passage_id && fs.corrected != Some(m.passage_id) {
            let local = crate::echo::AirPosition {
                passage_id: mine.passage_id,
                position_ms: mine.sample.saturating_mul(1000) / mine.rate.max(1) as u64,
                at: mine.heard_at,
            };
            // `local.at` is on THIS node's clock and `m.heard_at` on the
            // master's, so one is carried into the other's frame before they
            // are subtracted -- otherwise the offset between the clocks reads
            // as an alignment error `[GDE-ECHO-366]`.
            let local = crate::echo::AirPosition {
                at: fs.clock.now(local.at).unwrap_or(local.at),
                ..local
            };
            let residual = crate::echo::local_at_sample(&local, m) - m.heard_at as i64;
            // Every reading feeds the fit, not just the ones that trigger a
            // correction: the slope is what the rate trim runs on, and it
            // needs the whole series `[GDE-ECHO-340]`.
            fs.rate.push(now_master, residual);
            // **Added to what is already applied, not substituted for it**
            // `[GDE-ECHO-346]`. The residual being fitted is what remains
            // AFTER the current trim, so the slope is the error in the
            // correction rather than the drift. Sending it as an absolute
            // sets the trim to `R - A` when it already holds `A`: the fixed
            // point is half the drift and the map oscillates about it, which
            // leaves ~7 ppm of the measured 13.92 uncorrected for ever
            // `[LOG-P4-130]`.
            //
            // Applied once per window and then cleared, because the plant's
            // slope has just stepped and a line fitted across that step is not
            // a slope `[RateEstimate::clear]`.
            if let Some(ppm) = fs.rate.ppm() {
                fs.applied_ppm = crate::echo::next_trim_ppm(fs.applied_ppm, ppm);
                note(&mut fs.note, format!(
                    "echo-rate: {ppm:+.2} ppm still out; trimming at {:+.2} ppm",
                    fs.applied_ppm));
                handle.send(Command::SetEchoRate(fs.applied_ppm));
                fs.rate.clear();
            }
            // Every reading feeds the filter; the corrections read the
            // filter, never the reading `[GDE-ECHO-348]`.
            fs.filtered.push(now_master, residual);
            let Some(filtered) = fs.filtered.median() else {
                set_status(handle, &format!(
                    "Following, {:+.0} ms from that node (still measuring).",
                    residual as f64 / 1e6));
                return;
            };
            // Logged, not merely observed `[GDE-ECHO-260]`: a regression months
            // from now needs a baseline to fail against.
            set_status(handle, &format!("Following, {:+.0} ms from that node.",
                                        filtered as f64 / 1e6));

            // Below a mix quantum the boundary knobs cannot help: the coarse
            // step is bigger than the error `[GDE-ECHO-347]`. The frame trim
            // can, at 23 us a time `[GDE-ECHO-349]`.
            if filtered.unsigned_abs() <= OFFSET_ENDGAME.as_nanos() as u64
                && filtered.unsigned_abs() > OFFSET_DEADBAND.as_nanos() as u64
            {
                fs.filtered.clear();
                note(&mut fs.note, format!(
                    "echo-offset: {:+.0} ms out; shedding it by trimming frames",
                    filtered as f64 / 1e6));
                handle.send(Command::EchoShedOffset(filtered / 1_000_000));
                return;
            }

            match crate::echo::offset_fix(
                filtered, OFFSET_DEADBAND, OFFSET_MAX_BITE, OFFSET_REJOIN_BEYOND) {
                crate::echo::OffsetFix::Hold => {}
                crate::echo::OffsetFix::ShiftStart(ms) => {
                    fs.corrected = Some(m.passage_id);
                    // The correction is about to step the residual, and a step
                    // inside the window reads as an enormous slope. Clearing
                    // costs an hour of rate estimate and saves the loop from
                    // trimming hard against a drift that never happened.
                    fs.rate.clear();
                    fs.filtered.clear();
                    note(&mut fs.note, format!(
                        "echo-offset: {:+.0} ms out; starting the next passage {} ms {}",
                        filtered as f64 / 1e6, ms.abs(),
                        if ms > 0 { "earlier" } else { "later" }));
                    handle.send(Command::EchoCorrectNextStart(ms));
                }
                // Too far for one transition to absorb. Saying so beats a
                // silent hold -- this is the case a listener would otherwise
                // hear and not be able to explain `[GOV-SRC-040]`.
                crate::echo::OffsetFix::Rejoin => {
                    fs.corrected = Some(m.passage_id);
                    // The escape hatch for the suppression below: without this
                    // a node too far out would flow into every transition,
                    // never take a scheduled start, and stay out indefinitely.
                    fs.want_rejoin = true;
                    fs.filtered.clear();
                    // A rejoin places the first sample afresh, which steps the
                    // residual just as a nudge does.
                    fs.rate.clear();
                    note(&mut fs.note, format!(
                        "echo-offset: {:+.0} ms out, which is not an offset any more; waiting for a scheduled start", filtered as f64 / 1e6));
                }
            }
        }
    }

    match f.on_state(st, now_master) {
        Follow::Idle => {}
        Follow::Hold(why) => {
            set_status(handle, &format!(
                "The node being followed cannot place itself in time ({why:?}); holding."));
            note(&mut fs.note, format!("echo-follow: holding -- master reports {why:?}"));
        }
        Follow::Missed { passage_id } => {
            // Not a failure of the master's: a node with a large presentation
            // offset misses schedules a short-offset node makes comfortably
            // `[GDE-ECHO-410]`. Saying which node and which passage is what
            // makes that diagnosable instead of mysterious.
            set_status(handle, &format!(
                "Schedules are arriving too late for this speaker's {} ms delay; waiting for the next passage.", f.timing.offset().as_millis()));
            note(&mut fs.note, format!(
                "echo-follow: passage {passage_id} was already due for this node's \
{} ms offset; waiting for the next",
                f.timing.offset().as_millis()));
        }
        Follow::StartAt { passage_id, start_sample, at } => {
            // Already heading there, and heading there *at about the right
            // time*. Let it flow: a scheduled start goes through `skip`, which
            // cuts the ring `[REQ-AUD-158]`, and a follower that skips into
            // every passage loses its buffer at every boundary -- heard as a
            // stutter at the start of each track `[GDE-ECHO-336]`.
            //
            // **Being next is not enough on its own.** When the master skips,
            // its next passage is the one this node also has queued -- and
            // this node will not reach it for minutes. Suppressing on
            // queue membership alone is why a skip on the master left the
            // follower playing calmly on `[GDE-ECHO-353]`. The test is
            // whether flowing would land anywhere near the announced time.
            let flowing_would_do = match (own_transition_in_ms(handle), at.checked_sub(now_master)) {
                (Some(mine), Some(theirs)) => {
                    let theirs = theirs / 1_000_000;
                    mine.abs_diff(theirs) <= FLOW_TOLERANCE.as_millis() as u64
                }
                // Without both figures, believe what the master said rather
                // than a guess about this node's own future `[GOV-SRC-040]`.
                _ => false,
            };
            if next_up(handle) == Some(passage_id) && start_sample == 0
                && !fs.want_rejoin && flowing_would_do
            {
                note(&mut fs.note, format!(
                    "echo-follow: passage {passage_id} is already next here; flowing into it"));
                return;
            }
            fs.want_rejoin = false;
            fs.rate.clear();
            fs.filtered.clear();
            // **Committed.** A start is queued for a future instant, so the
            // passage is in neither `current` nor the queue until it fires --
            // which means `coming_here` says no and the mid-passage join
            // races it `[GDE-ECHO-354]`. Observed on a skip: the follower
            // took the master's corrected schedule at sample 0, then a second
            // later joined the same passage part-way in and threw it away.
            // Recording the commitment here is what makes the two paths see
            // each other.
            fs.mid_joined = Some((passage_id, std::time::Instant::now()));
            let at = fs.clock.to_local(at).unwrap_or(at);
            start(passage_id, start_sample, at, cfg, handle, &mut fs.note, "starting").await;
        }
    }
}

/// A passage, with the names a listener reads.
///
/// `Library::passage` returns the audio facts -- path, bounds, fades -- and
/// nothing a person would recognise; the title and artist come from a second
/// lookup against the recording's MBID. Every other queue path in the player
/// calls `describe`, and this one did not, so a follower's *Coming Up* listed
/// filenames where the node it followed listed songs.
fn named(lib: &crate::db::Library, passage_id: i64) -> Option<crate::queue::QueueEntry> {
    let mut e = lib.passage(passage_id).ok()?;
    lib.describe(&mut e);
    Some(e)
}

/// Take the followed node's queue as this node's own `[GDE-ECHO-500]`.
///
/// Only when it changes: the snapshot arrives twice a second and resolving a
/// queue means a database read per passage.
///
/// Passages this node does not have are dropped rather than refused wholesale.
/// Two libraries that have drifted apart should cost the passages they differ
/// on, not the whole programme `[GDE-ECHO-420]`.
async fn adopt_queue(
    snap: &Snapshot,
    cfg: &Following,
    handle: &Arc<EngineHandle>,
    fs: &mut FollowState,
) {
    let announced: Vec<i64> = snap.queue.iter().map(|e| e.passage_id).collect();
    if announced.is_empty() || announced == fs.queue {
        return;
    }
    fs.queue = announced.clone();
    let db = cfg.db.clone();
    let library = cfg.library.clone();
    let wanted = announced.len();
    let found = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open_split(&db, &library).ok()?;
        Some(announced.iter().filter_map(|id| named(&lib, *id)).collect::<Vec<_>>())
    })
    .await;
    if let Ok(Some(entries)) = found {
        if entries.len() < wanted {
            note(&mut fs.note, format!(
                "echo-queue: {} of {wanted} upcoming passages are in this node's library",
                entries.len()));
        }
        handle.send(Command::EchoSetQueue(entries));
    }
}

/// Resolve a passage against this node's own library and commit to the instant.
///
/// Both ways in land here -- a schedule for a passage about to start, and a
/// mid-passage catch-up -- because they differ only in which sample they name.
#[allow(clippy::too_many_arguments)]
async fn start(
    passage_id: i64,
    start_sample: u64,
    at: u64,
    cfg: &Following,
    handle: &Arc<EngineHandle>,
    last: &mut String,
    what: &str,
) {
    let db = cfg.db.clone();
    let library = cfg.library.clone();
    // The library is SQLite and blocking; the socket must not wait on a disk
    // read. Once per passage, so the spawn costs nothing.
    let found = tokio::task::spawn_blocking(move || {
        let lib = crate::db::Library::open_split(&db, &library)?;
        named(&lib, passage_id)
            .ok_or_else(|| crate::db::DbError::Query(format!("passage {passage_id} not here")))
    })
    .await;
    match found {
        Ok(Ok(entry)) => {
            set_status(handle, "Following.");
            eprintln!("echo-follow: {what} passage {passage_id} at sample {start_sample} in {:.3}s", (at as i64 - now_nanos() as i64) as f64 / 1e9);
            handle.send(Command::EchoStartAt { entry, start_sample, at_nanos: at });
        }
        // A master playing something this node does not have is the expected
        // cost of independent libraries, not an error to retry
        // `[GDE-ECHO-420]`. Reported once; the node carries on with its own
        // programme.
        Ok(Err(e)) => {
            set_status(handle, &format!(
                "That node is playing something this one does not have (passage {passage_id}); playing its own queue instead."));
            note(last, format!(
                "echo-follow: passage {passage_id} is not in this node's library ({e}); staying with its own queue"));
        }
        Err(e) => note(last, format!("echo-follow: library lookup failed: {e}")),
    }
}

/// Print only when the message changes.
///
/// The snapshot arrives twice a second forever, so an unconditional line for a
/// steady state would bury every line that matters. A log nobody can read is
/// the same as no log.
fn note(last: &mut String, line: String) {
    if line != *last {
        eprintln!("{line}");
        *last = line;
    }
}
