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
use vaino_player::mpd_backend::MpdBackend;
use vaino_player::playback::Playback;
use vaino_player::session::Session;

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

    let mut backend = MpdBackend::connect(&addr, &root, depth, interval).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    if write {
        match PlayerStore::open(Path::new(&db)) {
            Ok(s) => backend.attach_store(s),
            Err(e) => {
                eprintln!("cannot open {db} for writing: {e}");
                std::process::exit(1);
            }
        }
    }
    let caps = backend.capabilities();
    println!(
        "backend: MPD at {addr}, depth {depth}, sampling {interval}ms; \
         spans {} gain {} ramps {}",
        caps.spans, caps.gain, caps.ramps
    );
    println!("{}\n", if write { "WRITING plays and rejections" } else { "writes nothing" });
    if let Some(c) = session.census() {
        println!("pool: {} eligible, {} suppressed\n", c.eligible, c.suppressed);
    }

    let suppress = (saved.skip_suppress_h, saved.dequeue_suppress_h);
    let started = Instant::now();
    let mut last_reported = 0usize;
    while started.elapsed().as_secs_f64() < run_for {
        backend.tick();
        // The one line this program exists to run.
        session.refill(&mut backend, suppress);

        let n = backend.queued_ids().len();
        if n != last_reported {
            println!(
                "  queue: {n} passage(s) from the Director, {:.0}s of music ahead",
                backend.queued_ms() as f64 / 1000.0
            );
            last_reported = n;
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    println!("\nfinal queue: {:?}", backend.queued_ids());
    if let Some(c) = session.census() {
        println!(
            "pool now: {} eligible, {} suppressed, {} artist-blocked",
            c.eligible, c.suppressed, c.artist_blocked
        );
    }
}
