//! Play passages from `vaino.db` on the terminal — the headless sibling of
//! `vaino`, useful where a browser is not.
//!
//! Selection is the Program Director `[SPEC009]`, Stage A -- frequency alone,
//! until flavor distance lands and stages B and C can shape the pool.
//!
//! Usage:  station <vaino.db> [count] [--list]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vaino_player::engine::{Command, Engine};
use vaino_player::output::Output;
use vaino_player::queue::overlap_ms;
use vaino_player::session::Session;
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

    let mut session = match Session::open(&db, count) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("library: {} radio passages", session.lib.count_radio().unwrap_or(0));

    // Real device unless asked otherwise. A null sink cannot detect a
    // sample-rate fault [REQ-HW-147], so it must be opt-in, never the default.
    let out = if list_only || std::env::var("VAINO_NULL_OUTPUT").is_ok() {
        if !list_only {
            println!("output: null sink");
        }
        None
    } else {
        match Output::open(BUFFER_FRAMES * 2) {
            Ok(o) => {
                println!("output: {} @ {} Hz, {} ch", o.device_name, o.sample_rate, o.channels);
                Some(o)
            }
            Err(e) => {
                eprintln!("no audio device ({e}); using null sink");
                None
            }
        }
    };

    // Why the pool is the size it is [SPEC-DIR-190] -- where a station that has
    // gone quiet is diagnosed. Taken BEFORE priming, deliberately: queueing
    // updates the Director's history, so a census afterwards would describe the
    // pool minus what was just queued, which is a different question.
    if let Some(c) = session.census() {
        println!("pool: {} eligible, total weight {:.1}", c.eligible, c.total_weight);
        println!("      blocked: {} artist, {} track, {} related | {} under min weight, {} filtered",
                 c.artist_blocked, c.track_blocked, c.related_blocked,
                 c.below_min_weight, c.filtered);
    } else {
        println!("pool: program director unavailable; random selection");
    }

    let (mut engine, handle) = Engine::new(out, count);
    session.prime(&mut engine);

    // List what the engine will actually play, not a second draw from the
    // library: a preview that re-randomised would describe a different evening.
    let entries: Vec<_> = engine.queued().cloned().collect();
    println!("queued {}\n", entries.len());
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
        // Unlike `vaino`, this plays a fixed set and stops -- it is a test
        // harness, not the appliance.
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
