//! Stage 3: the Program Director drives MPD `[IMPL-MPD-040]`.
//!
//!   mpd_direct [host:port] --db vaino.db --root MUSIC_DIRECTORY
//!              [--depth 5] [--interval 5] [--for SECONDS] [--seed N]
//!
//! Stage 2 proved `rangeid` under a deliberately stupid selector. This swaps
//! that selector for `Director::decide` — the same call `Session::refill` makes
//! locally, with the same exclusion set and the same flow tail `[SPEC-DIR-160]`.
//! The selection code is *not* reimplemented here; that is the point.
//!
//! **The bookkeeping is the part to get right, not the selection**
//! `[IMPL-MPD-045]`. Locally a passage that never played is undone through
//! `take_dropped`; here the only evidence is that a song id left MPD's queue,
//! and the same event means two different things depending on whether it was
//! ever the current song.
//!
//! **Writes nothing to the database.** Stage 4 does that `[IMPL-MPD-050]`. What
//! would be recorded is printed, so the bookkeeping can be judged before it is
//! given the power to be wrong in a file.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use vaino_player::director::library::{Director, QueuedNote, Rng};
use vaino_player::mpd::{parse, Mpd};

/// What the Director handed over, and what MPD did with it.
struct Offered {
    passage_id: i64,
    title: String,
    /// The undo for `note_queued`, held until the passage's fate is known
    /// `[REQ-PD-112]`.
    note: Option<QueuedNote>,
    /// Whether this song has ever been the current one. It is the whole
    /// difference between "the listener heard it" and "the listener removed
    /// it", and MPD does not report it — only the sampler can know.
    was_current: bool,
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(db_path), Some(root)) = (flag(&args, "--db"), flag(&args, "--root")) else {
        eprintln!("usage: mpd_direct [host:port] --db vaino.db --root MUSIC_DIRECTORY");
        eprintln!("       [--depth 5] [--interval 5] [--for SECONDS] [--seed N]");
        std::process::exit(2);
    };
    let depth: usize = flag(&args, "--depth").and_then(|v| v.parse().ok()).unwrap_or(5);
    let interval: f64 = flag(&args, "--interval").and_then(|v| v.parse().ok()).unwrap_or(5.0);
    let run_for: Option<f64> = flag(&args, "--for").and_then(|v| v.parse().ok());
    let flagged: HashSet<&str> =
        ["--db", "--root", "--depth", "--interval", "--for", "--seed"].into_iter().collect();
    let taken: HashSet<usize> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| flagged.contains(a.as_str()))
        .map(|(i, _)| i + 1)
        .collect();
    let addr = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && !taken.contains(i))
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| "127.0.0.1:6600".into());
    let mut rng = match flag(&args, "--seed").and_then(|v| v.parse().ok()) {
        Some(s) => Rng::seeded(s),
        None => Rng::from_clock(),
    };

    let conn = Connection::open(&db_path).unwrap_or_else(|e| {
        eprintln!("{db_path}: {e}");
        std::process::exit(1);
    });
    let started_load = Instant::now();
    let mut director = Director::load(&conn).unwrap_or_else(|e| {
        eprintln!("director: {e}");
        std::process::exit(1);
    });
    let root_norm = root.replace('\\', "/").trim_end_matches('/').to_string();

    let mut mpd = Mpd::connect(&addr).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    println!("MPD protocol {} at {addr}", mpd.version);
    println!("director loaded in {:.1}s", started_load.elapsed().as_secs_f64());
    if let Some(c) = Some(director.census(now())) {
        println!(
            "pool: {} eligible, {} suppressed by a recent skip or removal",
            c.eligible, c.suppressed
        );
    }
    println!("depth {depth}, sampling {interval}s; writes nothing\n");

    if let Err(e) = mpd.cmd("consume 1") {
        eprintln!("consume: {e}");
        std::process::exit(1);
    }

    let started = Instant::now();
    let mut ours: HashMap<String, Offered> = HashMap::new();
    let (mut added, mut played, mut removed, mut unhonoured) = (0u32, 0u32, 0u32, 0u32);

    loop {
        let status = match mpd.status() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("status: {e}");
                break;
            }
        };
        let state = status.get("state").cloned().unwrap_or_default();
        let current = status.get("songid").cloned();
        // Only while playing `[SPEC-MPD-120]`.
        let active = state == "play";

        // Mark the current song before diffing, so a passage that started
        // between two samples is not mistaken for one removed by hand.
        if let Some(id) = &current {
            if let Some(o) = ours.get_mut(id) {
                o.was_current = true;
            }
        }

        // --- the queue diff, which is this stage's real work ---
        let live: HashSet<String> = match mpd.cmd("playlistid") {
            Ok(lines) => lines
                .iter()
                .filter_map(|l| l.strip_prefix("Id: "))
                .map(|s| s.to_string())
                .collect(),
            Err(e) => {
                eprintln!("playlistid: {e}");
                break;
            }
        };
        let gone: Vec<String> = ours.keys().filter(|id| !live.contains(*id)).cloned().collect();
        for id in gone {
            let Some(o) = ours.remove(&id) else { continue };
            if o.was_current {
                // It reached the front, so it played or was skipped. Either way
                // the listener met it, and the note stands — the same as
                // locally, where only an unopenable passage is undone.
                played += 1;
                println!("  played or skipped [{}] {}", o.passage_id, o.title);
            } else {
                // It never played and it left: a person took it out. Undo the
                // queueing mark `[REQ-PD-112]` so the Director does not go on
                // believing it was heard...
                removed += 1;
                if let Some(note) = o.note {
                    director.forget_queued(note);
                }
                // ...and record the removal as the weaker rejection, which is a
                // separate mechanism with a separate window `[SPEC-PLAY-055]`.
                // Undoing the note is not the same as forgiving the removal.
                println!(
                    "  removed by hand [{}] {} -- queue mark undone; would record a dequeue",
                    o.passage_id, o.title
                );
            }
        }

        let len = live.len();
        if !active || len >= depth {
            if run_for.is_some_and(|s| started.elapsed().as_secs_f64() >= s) {
                break;
            }
            std::thread::sleep(Duration::from_secs_f64(interval));
            continue;
        }

        // --- selection, which is not reimplemented ---
        for _ in 0..(depth - len) {
            // The exclusion set is what is already queued, and the flow tail is
            // the last of them `[SPEC-DIR-160]` — exactly `Session::refill`.
            let skip: Vec<i64> = ours.values().map(|o| o.passage_id).collect();
            let after = ours.values().last().map(|o| o.passage_id);
            let Some(decision) = director.decide(now(), &mut rng, &skip, after) else {
                println!("  director has nothing eligible to offer");
                break;
            };
            let e = &decision.entry;
            let uri = {
                let norm = e.path.to_string_lossy().replace('\\', "/");
                match norm.strip_prefix(&root_norm) {
                    Some(r) => r.trim_start_matches('/').to_string(),
                    None => {
                        println!("  [{}] does not resolve under the music root", e.passage_id);
                        continue;
                    }
                }
            };
            match mpd.add_ranged(&uri, e.start_ms as i64, e.end_ms as i64) {
                Ok(a) => {
                    if !a.span_honoured {
                        unhonoured += 1;
                        println!("  ! span not honoured for [{}]", e.passage_id);
                    }
                    let note = director.note_queued(e.passage_id, now());
                    let title = e.title();
                    println!("  + [{:>6}] {}", e.passage_id, title);
                    ours.insert(
                        a.id,
                        Offered { passage_id: e.passage_id, title, note, was_current: false },
                    );
                    added += 1;
                }
                Err(err) => println!("  {err}"),
            }
        }

        if run_for.is_some_and(|s| started.elapsed().as_secs_f64() >= s) {
            break;
        }
        std::thread::sleep(Duration::from_secs_f64(interval));
    }

    println!("\noffered {added}; {played} reached the front; {removed} removed by hand");
    if unhonoured > 0 {
        println!("{unhonoured} span(s) MPD would not honour `[SPEC-MPD-096]`");
    }
    let c = director.census(now());
    println!(
        "pool now: {} eligible, {} suppressed, {} track-blocked, {} artist-blocked",
        c.eligible, c.suppressed, c.track_blocked, c.artist_blocked
    );
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
