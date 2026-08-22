//! The whole of `[SPEC018]` in one program: Vaino's own session, playing MPD.
//!
//!   mpd_session [host:port] --db vaino.db --root MUSIC_DIRECTORY
//!               [--depth N] [--interval MS] [--for SECONDS] [--write]
//!
//! **Nothing here selects anything.** It opens a `Session` — the same one
//! `vaino` runs — hands it an [`MpdBackend`] instead of an `Engine`, and calls
//! `refill`. If passages appear in MPD's queue, then the Director, its
//! rotation, its flow and its bookkeeping all reached a backend that is not the
//! built-in one, through the seam and without knowing they had `[SPEC-BK-022]`.
//!
//! `mpd_direct` proved the same behaviour by reimplementing the refill loop.
//! This proves it by *not* reimplementing it, which is the stronger claim and
//! the reason this binary exists alongside that one.

use std::path::Path;
use std::time::{Duration, Instant};

use vaino_player::db::PlayerStore;
use vaino_player::engine::Engine;
use vaino_player::mpd_backend::MpdBackend;
use vaino_player::playback::Playback;
use vaino_player::session::Session;
use vaino_player::switch::{Side, Stopped, Switching};

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(db), Some(root)) = (flag(&args, "--db"), flag(&args, "--root")) else {
        eprintln!("usage: mpd_session [host:port] --db vaino.db --root MUSIC_DIRECTORY");
        eprintln!("       [--depth N] [--interval MS] [--for SECONDS] [--write]");
        std::process::exit(2);
    };
    let write = args.iter().any(|a| a == "--write");
    let run_for: f64 = flag(&args, "--for").and_then(|v| v.parse().ok()).unwrap_or(30.0);
    let addr = args
        .iter()
        .find(|a| a.contains(':') && !a.starts_with("--") && !a.contains('/'))
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:6600".into());

    // The listener owns the depth and the sampling rate `[SPEC-MPD-105]`.
    let saved = PlayerStore::open(Path::new(&db))
        .ok()
        .and_then(|s| s.load_settings())
        .unwrap_or_default();
    let depth: usize =
        flag(&args, "--depth").and_then(|v| v.parse().ok()).unwrap_or(saved.queue_depth);
    let interval: u64 = flag(&args, "--interval")
        .and_then(|v| v.parse().ok())
        .unwrap_or(saved.sample_interval_ms);

    let loading = Instant::now();
    let mut session = Session::open(Path::new(&db), depth).unwrap_or_else(|e| {
        eprintln!("session: {e}");
        std::process::exit(1);
    });
    println!("session opened in {:.1}s", loading.elapsed().as_secs_f64());

    let mut guest = MpdBackend::connect(&addr, &root, depth, interval).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if write {
        match PlayerStore::open(Path::new(&db)) {
            Ok(s) => guest.attach_store(s),
            Err(e) => {
                eprintln!("cannot open {db} for writing: {e}");
                std::process::exit(1);
            }
        }
    }

    // A *real* engine, with a silent output. This is the local backend the
    // appliance would be using; only the sound card is absent, so a queue that
    // arrives here has arrived somewhere that could play it.
    let (engine, _handle) = Engine::new(vaino_player::path::PathHandle::silent(), depth);
    let mut sw = Switching::new(Box::new(engine));
    sw.attach_guest(Box::new(guest));
    sw.switch_to(Side::Guest).expect("guest attached");

    println!(
        "backend: MPD at {addr}, depth {depth}, sampling {interval}ms; spans {} gain {} ramps {}",
        sw.capabilities().spans,
        sw.capabilities().gain,
        sw.capabilities().ramps
    );
    println!("{}
", if write { "WRITING plays and rejections" } else { "writes nothing" });
    if let Some(c) = session.census() {
        println!("pool: {} eligible, {} suppressed
", c.eligible, c.suppressed);
    }

    let suppress = (saved.skip_suppress_h, saved.dequeue_suppress_h);
    let started = Instant::now();
    let mut last = 0usize;
    while started.elapsed().as_secs_f64() < run_for {
        sw.tick();
        session.refill(&mut sw, suppress);
        let n = sw.queued_ids().len();
        if n != last {
            println!("  MPD queue: {n} passage(s), {:.0}s ahead", sw.queued_ms() as f64 / 1000.0);
            last = n;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // --- the handoff `[SPEC-BK-030]` ---
    println!("
handing MPD -> Vaino");
    let before = sw.queued_ids();

    println!("  MPD was holding {} passage(s): {:?}", before.len(), before);
    match session.hand_over_over(&mut sw, Side::Local, 600) {
        Ok((c, stopped)) => {
            println!(
                "  MPD {}",
                match stopped {
                    Stopped::Faded => "faded out over 600ms",
                    Stopped::Cut => "CUT -- no mixer on its output, so no fade was possible",
                }
            );
            println!("  carried {} passage(s) to the local engine: {:?}", c.moved.len(), c.moved);
            if !c.lost.is_empty() {
                println!("  the library could no longer build: {:?}", c.lost);
            }
        }
        Err(e) => println!("  refused: {e}"),
    }
    println!("  now active: {:?}", sw.active());
    println!("  local engine holds {} passage(s): {:?}", sw.queued_ids().len(), sw.queued_ids());
    println!(
        "  capabilities now: gain {} ramps {} (the local side can do what MPD cannot)",
        sw.capabilities().gain,
        sw.capabilities().ramps
    );
}
