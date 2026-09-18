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
const OFFSET_DEADBAND: Duration = Duration::from_millis(40);

/// The most a single transition may be asked to absorb.
///
/// Beyond this the overlap would have to grow past the audio that exists or
/// shrink through zero into a gap, and placing the first sample afresh is both
/// cleaner and, at that size, no longer inaudible anyway `[GDE-ECHO-340]`.
const OFFSET_MAX_HIDDEN: Duration = Duration::from_millis(250);

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
    /// The master passage a mid-passage join was last attempted for.
    mid_joined: Option<i64>,
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
        }
    }
}

/// The passage this node will play next of its own accord.
///
/// `None` when nothing is queued, which is the case a scheduled start exists
/// for.
fn next_up(handle: &EngineHandle) -> Option<i64> {
    handle.state.lock().ok()?.queue.first().map(|e| e.passage_id)
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
    // Catching up to what the master is ALREADY playing, which the schedule
    // cannot do: a schedule describes a passage about to start, and the moment
    // somebody switches a speaker into follower mode is almost never one
    // `[SPEC-ECHO-030]`.
    //
    // Attempted once per master passage. A node whose library lacks it must
    // not retry twice a second forever, and when the master moves on the
    // ordinary schedule path takes over anyway.
    let (join_now, playing) = join_intent(handle);
    if join_now {
        if let Some(a) = st.anchor.as_ref() {
            let already = playing == Some(a.passage_id) || fs.mid_joined == Some(a.passage_id);
            if !already {
                fs.mid_joined = Some(a.passage_id);
                let air = crate::echo::AirPosition {
                    passage_id: a.passage_id,
                    position_ms: a.sample.saturating_mul(1000) / a.rate.max(1) as u64,
                    at: a.heard_at,
                };
                let lead = f.timing.offset() + MID_JOIN_MARGIN;
                if let Some(j) = crate::echo::join_mid_passage(&air, f.timing, now_nanos(), lead) {
                    start(j.passage_id, j.start_sample, j.submit_at, cfg, handle, &mut fs.note,
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
            let residual = crate::echo::local_at_sample(&local, m) - m.heard_at as i64;
            // Every reading feeds the fit, not just the ones that trigger a
            // correction: the slope is what the rate trim runs on, and it
            // needs the whole series `[GDE-ECHO-340]`.
            fs.rate.push(now_nanos(), residual);
            if let Some(ppm) = fs.rate.ppm() {
                handle.send(Command::SetEchoRate(ppm));
            }
            // Logged, not merely observed `[GDE-ECHO-260]`: a regression months
            // from now needs a baseline to fail against.
            set_status(handle, &format!("Following, {:+.0} ms from that node.",
                                        residual as f64 / 1e6));
            match crate::echo::offset_fix(residual, OFFSET_DEADBAND, OFFSET_MAX_HIDDEN) {
                crate::echo::OffsetFix::Hold => {}
                crate::echo::OffsetFix::ShiftStart(ms) => {
                    fs.corrected = Some(m.passage_id);
                    // The correction is about to step the residual, and a step
                    // inside the window reads as an enormous slope. Clearing
                    // costs an hour of rate estimate and saves the loop from
                    // trimming hard against a drift that never happened.
                    fs.rate.clear();
                    note(&mut fs.note, format!(
                        "echo-offset: {:+.0} ms out; starting the next passage {} ms {}",
                        residual as f64 / 1e6, ms.abs(),
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
                    // A rejoin places the first sample afresh, which steps the
                    // residual just as a nudge does.
                    fs.rate.clear();
                    note(&mut fs.note, format!(
                        "echo-offset: {:+.0} ms out, more than one transition can absorb; waiting for a scheduled start", residual as f64 / 1e6));
                }
            }
        }
    }

    match f.on_state(st, now_nanos()) {
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
            // Already heading there. Let it flow: a scheduled start goes
            // through `skip`, which cuts the ring `[REQ-AUD-158]`, and a
            // follower that skips into every passage loses its buffer at every
            // boundary -- heard as a stutter at the start of each track. The
            // offset it would have corrected is what the overlap is for
            // `[GDE-ECHO-340]`.
            if next_up(handle) == Some(passage_id) && start_sample == 0 && !fs.want_rejoin {
                note(&mut fs.note, format!(
                    "echo-follow: passage {passage_id} is already next here; flowing into it"));
                return;
            }
            fs.want_rejoin = false;
            fs.rate.clear();
            start(passage_id, start_sample, at, cfg, handle, &mut fs.note, "starting").await;
        }
    }
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
        Some(announced.iter().filter_map(|id| lib.passage(*id).ok()).collect::<Vec<_>>())
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
        crate::db::Library::open_split(&db, &library).and_then(|lib| lib.passage(passage_id))
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
