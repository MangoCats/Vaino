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
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        let mut follower =
            Follower::new(cfg.timing, Duration::from_micros(500), Duration::from_secs(1));
        // Per connection: a node that reconnects should catch up again, since
        // whatever it was playing while disconnected is by then its own.
        let mut mid_joined: Option<i64> = None;
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
            act(&mut follower, &snap.echo, &cfg, &handle, &mut last_note, &mut mid_joined).await;
        }
        set_status(&handle, "Not connected.");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn act(
    f: &mut Follower,
    st: &EchoState,
    cfg: &Following,
    handle: &Arc<EngineHandle>,
    last: &mut String,
    mid_joined: &mut Option<i64>,
) {
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
            let already = playing == Some(a.passage_id) || *mid_joined == Some(a.passage_id);
            if !already {
                *mid_joined = Some(a.passage_id);
                let air = crate::echo::AirPosition {
                    passage_id: a.passage_id,
                    position_ms: a.sample.saturating_mul(1000) / a.rate.max(1) as u64,
                    at: a.heard_at,
                };
                let lead = cfg.timing.offset() + MID_JOIN_MARGIN;
                if let Some(j) = crate::echo::join_mid_passage(&air, cfg.timing, now_nanos(), lead) {
                    start(j.passage_id, j.start_sample, j.submit_at, cfg, handle, last,
                          "joining part-way into").await;
                }
            }
        }
    }

    match f.on_state(st, now_nanos()) {
        Follow::Idle => {}
        Follow::Hold(why) => {
            set_status(handle, &format!(
                "The node being followed cannot place itself in time ({why:?}); holding."));
            note(last, format!("echo-follow: holding -- master reports {why:?}"));
        }
        Follow::Missed { passage_id } => {
            // Not a failure of the master's: a node with a large presentation
            // offset misses schedules a short-offset node makes comfortably
            // `[GDE-ECHO-410]`. Saying which node and which passage is what
            // makes that diagnosable instead of mysterious.
            set_status(handle, &format!(
                "Schedules are arriving too late for this speaker's {} ms delay; waiting for the next passage.", cfg.timing.offset().as_millis()));
            note(last, format!(
                "echo-follow: passage {passage_id} was already due for this node's \
{} ms offset; waiting for the next",
                cfg.timing.offset().as_millis()));
        }
        Follow::StartAt { passage_id, start_sample, at } => {
            start(passage_id, start_sample, at, cfg, handle, last, "starting").await;
        }
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
