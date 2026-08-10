//! Play passages from `vaino.db` — the first binary that is a player rather
//! than a demo, joining P1's data to P2's audio chain.
//!
//! Selection is random radio passages for now; the Program Director `[SPEC009]`
//! replaces that without touching anything below.
//!
//! Usage:  station <vaino.db> [count] [--list]

use std::path::PathBuf;

use std::time::{Duration, Instant};

use vaino_player::db::{Library, PlayerStore};
use vaino_player::engine::{Command, Engine};
use vaino_player::output::Output;
use vaino_player::queue::{overlap_ms, Queue};
use vaino_player::BUFFER_FRAMES;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: station <vaino.db> [count] [--list]");
        std::process::exit(2);
    }
    let db = PathBuf::from(&args[0]);
    let count: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let list_only = args.iter().any(|a| a == "--list");

    let lib = match Library::open(&db) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("library: {} radio passages", lib.count_radio().unwrap_or(0));

    // Where playback left off [REQ-AUD-140]. A missing or unreadable resume
    // point is not an error -- it is a first run -- so it degrades to a fresh
    // start rather than refusing to play.
    let store = PlayerStore::open(&db)
        .map_err(|e| eprintln!("resume state unavailable ({e}); continuing without it"))
        .ok();
    let resume = store.as_ref().and_then(|s| s.load().ok()).flatten();

    let mut q = Queue::new(count);
    // The resumed passage goes first; the rest of the queue fills behind it.
    let mut resume_ms = 0;
    if let Some((Some(pid), pos, _)) = resume {
        match lib.passage(pid) {
            Ok(e) => {
                println!("resuming passage {pid} at {:.1}s", pos as f64 / 1000.0);
                resume_ms = pos;
                q.push(e);
            }
            // The library was rebuilt and the passage renumbered away: start fresh.
            Err(_) => eprintln!("saved passage {pid} is no longer in the library"),
        }
    }
    match lib.random_radio(count.saturating_sub(q.len())) {
        Ok(entries) => entries.into_iter().for_each(|e| q.push(e)),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
    println!("queued {} (shortfall {})\n", q.len(), q.shortfall());

    let entries: Vec<_> = q.iter().cloned().collect();
    for (i, e) in entries.iter().enumerate() {
        let name = e.path.file_name().unwrap_or_default().to_string_lossy();
        let missing = if e.path.exists() { "" } else { "  [FILE MISSING]" };
        println!("{:>2}. {:<44} {:>6.1}s  lead {}/{} ms  {:+.1} dB{}",
                 i + 1, &name.chars().take(44).collect::<String>(),
                 e.duration_ms() as f64 / 1000.0,
                 e.lead_in_ms, e.lead_out_ms, e.gain_db, missing);
        if let Some(next) = entries.get(i + 1) {
            println!("     -> crossfade {:.1}s", overlap_ms(e, next) as f64 / 1000.0);
        }
    }
    if list_only {
        return;
    }
    // Real device unless asked otherwise. A null sink cannot detect a
    // sample-rate fault [REQ-HW-147], so it must be opt-in, never the default.
    let out = if std::env::var("VAINO_NULL_OUTPUT").is_ok() {
        println!("\noutput: null sink");
        None
    } else {
        match Output::open(BUFFER_FRAMES * 2) {
            Ok(o) => {
                println!("\noutput: {} @ {} Hz, {} ch", o.device_name, o.sample_rate, o.channels);
                Some(o)
            }
            Err(e) => {
                eprintln!("\nno audio device ({e}); using null sink");
                None
            }
        }
    };

    let (mut engine, handle) = Engine::new(out, count);
    if let Some(s) = store {
        engine.attach_store(s);
    }
    if resume_ms > 0 {
        engine.resume_at(resume_ms);
    }
    entries.iter().for_each(|e| engine.enqueue(e.clone()));
    handle.send(Command::Play);

    let started = Instant::now();
    let mut last_id = -1i64;
    while !engine.is_shutdown() {
        let submitted = engine.tick();
        let s = handle.snapshot();
        if let Some(c) = &s.current {
            if c.passage_id != last_id {
                last_id = c.passage_id;
                println!(">> {}", c.path.file_name().unwrap_or_default().to_string_lossy());
            }
        }
        if s.is_idle() {
            break;
        }
        // Without a device there is no back-pressure, so pace the loop rather
        // than spinning a core flat out.
        if submitted == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let s = handle.snapshot();
    println!("\nfinished in {:.1}s | underrun samples: {}",
             started.elapsed().as_secs_f64(), s.underrun_samples);
}
