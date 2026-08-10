//! Play arbitrary files, for audio not in the library.
//!
//! Uses the same [`Engine`] as `station`; it differs only in where passages
//! come from. It previously carried its own pump loop, which duplicated the
//! engine and then diverged from it — the engine gained back-pressure handling
//! this copy lacked, so identical audio behaved differently depending on which
//! binary played it.
//!
//! Usage:
//!     play <file> [start_ms] [end_ms]
//!     play <fileA> <startA> <endA> <fileB> <startB> <endB> [lead_s]
//!
//! Set VAINO_NULL_OUTPUT=1 for a discard sink (headless/CI). It accepts
//! everything instantly, so it cannot reveal rate or back-pressure faults
//! `[REQ-HW-147]`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vaino_player::engine::{Command, Engine};
use vaino_player::output::Output;
use vaino_player::queue::QueueEntry;
use vaino_player::BUFFER_FRAMES;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.is_empty() {
        eprintln!("usage: play <file> [start_ms] [end_ms]");
        eprintln!("       play <fA> <sA> <eA> <fB> <sB> <eB> [lead_s]");
        std::process::exit(2);
    }

    // A lead is needed on BOTH sides: overlap is min(lead_out(A), lead_in(B)),
    // so setting only one yields no crossfade at all.
    let lead_ms = (a.get(6).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0) * 1000.0) as u64;
    let mut entries: Vec<QueueEntry> = Vec::new();
    {
        let mut add = |path: &str, start: u64, end: u64, lin: u64, lout: u64| {
            entries.push(QueueEntry {
                passage_id: entries.len() as i64,
                path: PathBuf::from(path),
                start_ms: start,
                end_ms: end,
                lead_in_ms: lin,
                lead_out_ms: lout,
                gain_db: 0.0,
            });
        };
        if a.len() >= 6 {
            add(&a[0], a[1].parse().unwrap_or(0), a[2].parse().unwrap_or(u64::MAX), 0, lead_ms);
            add(&a[3], a[4].parse().unwrap_or(0), a[5].parse().unwrap_or(u64::MAX), lead_ms, 0);
        } else {
            let start = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let end = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
            add(&a[0], start, end, 0, 0);
        }
    }

    let out = if std::env::var("VAINO_NULL_OUTPUT").is_ok() {
        println!("output: null sink");
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
    let rate = out.as_ref().map(|o| o.sample_rate).unwrap_or(44_100);
    let channels = out.as_ref().map(|o| o.channels).unwrap_or(2);

    let (mut engine, handle) = Engine::new(out, entries.len());
    for e in &entries {
        println!("passage: {} [{}..{}] lead {}/{} ms",
                 e.path.file_name().unwrap_or_default().to_string_lossy(),
                 e.start_ms, e.end_ms, e.lead_in_ms, e.lead_out_ms);
        engine.enqueue(e.clone());
    }
    handle.send(Command::Play);

    let t0 = Instant::now();
    let mut submitted: u64 = 0;
    while !engine.is_stopped() {
        let n = engine.tick();
        submitted += n as u64;
        let s = handle.snapshot();
        if s.is_idle() {
            break;
        }
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let under = handle.snapshot().underrun_samples;
    let audio_s = submitted as f64 / (rate as f64 * channels as f64);
    println!("\nsubmitted {audio_s:.1}s of audio in {:.1}s wall | underrun samples {under}",
             t0.elapsed().as_secs_f64());
}
