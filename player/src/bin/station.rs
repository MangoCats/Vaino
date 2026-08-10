//! Play passages from `vaino.db` — the first binary that is a player rather
//! than a demo, joining P1's data to P2's audio chain.
//!
//! Selection is random radio passages for now; the Program Director `[SPEC009]`
//! replaces that without touching anything below.
//!
//! Usage:  station <vaino.db> [count] [--list]

use std::path::PathBuf;

use vaino_player::db::Library;
use vaino_player::queue::{overlap_ms, Queue};

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

    let mut q = Queue::new(count);
    match lib.random_radio(count) {
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
    println!("\n(playback wiring lands with the engine loop; --list shows the schedule)");
}
