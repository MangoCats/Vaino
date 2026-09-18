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
    pub url: String,
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
pub async fn run(cfg: Following, handle: Arc<EngineHandle>) {
    eprintln!("echo-follow: following {} with offset {} frames ({} ms)",
              cfg.url, cfg.timing.presentation_offset_frames,
              cfg.timing.offset().as_millis());
    let mut follower = Follower::new(cfg.timing, Duration::from_micros(500), Duration::from_secs(1));
    // A follower's basis would normally be established and voided by its own
    // audio path. Nothing here trims, so nothing consults it; it is
    // established once and left alone, and that is said out loud because a
    // node that DOES trim must not do this `[GDE-ECHO-360]`.
    follower.basis.establish();
    let mut last_note = String::new();

    loop {
        let ws = match tokio_tungstenite::connect_async(&cfg.url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                note(&mut last_note, format!("echo-follow: connect failed: {e}; retrying in 3s"));
                tokio::time::sleep(Duration::from_secs(3)).await;
                continue;
            }
        };
        note(&mut last_note, format!("echo-follow: connected to {}", cfg.url));
        let (_, mut rx) = ws.split();

        while let Some(msg) = rx.next().await {
            let text = match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => t,
                Ok(_) => continue,
                Err(e) => {
                    note(&mut last_note, format!("echo-follow: socket error: {e}"));
                    break;
                }
            };
            let Ok(snap) = serde_json::from_str::<Snapshot>(&text) else { continue };
            act(&mut follower, &snap.echo, &cfg, &handle, &mut last_note).await;
        }
        note(&mut last_note, "echo-follow: disconnected; retrying in 3s".to_string());
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn act(
    f: &mut Follower,
    st: &EchoState,
    cfg: &Following,
    handle: &Arc<EngineHandle>,
    last: &mut String,
) {
    match f.on_state(st, now_nanos()) {
        Follow::Idle => {}
        Follow::Hold(why) => {
            note(last, format!("echo-follow: holding -- master reports {why:?}"));
        }
        Follow::Missed { passage_id } => {
            // Not a failure of the master's: a node with a large presentation
            // offset misses schedules a short-offset node makes comfortably
            // `[GDE-ECHO-410]`. Saying which node and which passage is what
            // makes that diagnosable instead of mysterious.
            note(last, format!(
                "echo-follow: passage {passage_id} was already due for this node's \
{} ms offset; waiting for the next",
                cfg.timing.offset().as_millis()));
        }
        Follow::StartAt { passage_id, start_sample, at } => {
            let db = cfg.db.clone();
            let library = cfg.library.clone();
            // The library is SQLite and blocking; the socket must not wait on
            // a disk read. Once per passage, so the spawn costs nothing.
            let found = tokio::task::spawn_blocking(move || {
                crate::db::Library::open_split(&db, &library)
                    .and_then(|lib| lib.passage(passage_id))
            }).await;
            match found {
                Ok(Ok(entry)) => {
                    eprintln!("echo-follow: starting passage {passage_id} at sample \
{start_sample} in {:.3}s", (at as i64 - now_nanos() as i64) as f64 / 1e9);
                    handle.send(Command::EchoStartAt { entry, start_sample, at_nanos: at });
                }
                // A master playing something this node does not have is the
                // expected cost of independent libraries, not an error to
                // retry `[GDE-ECHO-420]`. It is reported once and the node
                // carries on with its own programme.
                Ok(Err(e)) => note(last, format!(
                    "echo-follow: passage {passage_id} is not in this node's library ({e}); \
staying with its own queue")),
                Err(e) => note(last, format!("echo-follow: library lookup failed: {e}")),
            }
        }
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
