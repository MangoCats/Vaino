//! End-to-end proof of the audio chain: decode -> fade -> mix -> output.
//!
//! Plays one passage, or crossfades two, exercising every component together.
//! The point is not the CLI but the pump loop below, which is the shape the
//! real engine will take.
//!
//! Usage:
//!     play <file> [start_ms] [end_ms]                       -- one passage
//!     play <fileA> <startA> <endA> <fileB> <startB> <endB> [overlap_s]
//!
//! With no audio device (CI, headless), set VAINO_NULL_OUTPUT=1 to run the same
//! pipeline into a discard sink, which still exercises decode/fade/mix.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vaino_player::decoder::PassageDecoder;
use vaino_player::fade::{Curve, Fade};
use vaino_player::mixer::{mix, Stream};
use vaino_player::queue::{should_admit, QueueEntry};
use vaino_player::output::Output;
use vaino_player::BUFFER_FRAMES;

/// One passage to play: where it is, and how it enters and leaves.
struct Cue {
    path: PathBuf,
    start_ms: u64,
    end_ms: Option<u64>,
    fade_in_s: f32,
    fade_out_s: f32,
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.is_empty() {
        eprintln!("usage: play <file> [start_ms] [end_ms]");
        eprintln!("       play <fA> <sA> <eA> <fB> <sB> <eB> [overlap_s]");
        std::process::exit(2);
    }

    let overlap = a.get(6).and_then(|s| s.parse::<f32>().ok()).unwrap_or(4.0);
    let cues: Vec<Cue> = if a.len() >= 6 {
        vec![
            Cue { path: PathBuf::from(&a[0]), start_ms: a[1].parse().unwrap_or(0),
                  end_ms: a[2].parse().ok(), fade_in_s: 0.0, fade_out_s: overlap },
            Cue { path: PathBuf::from(&a[3]), start_ms: a[4].parse().unwrap_or(0),
                  end_ms: a[5].parse().ok(), fade_in_s: overlap, fade_out_s: 0.0 },
        ]
    } else {
        vec![Cue { path: PathBuf::from(&a[0]),
                   start_ms: a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                   end_ms: a.get(2).and_then(|s| s.parse().ok()),
                   fade_in_s: 0.5, fade_out_s: 0.5 }]
    };

    let null_out = std::env::var("VAINO_NULL_OUTPUT").is_ok();
    let out = if null_out {
        println!("output: null sink (VAINO_NULL_OUTPUT)");
        None
    } else {
        match Output::open(BUFFER_FRAMES * 2) {
            Ok(o) => {
                println!("output: {} Hz, {} ch", o.sample_rate, o.channels);
                Some(o)
            }
            Err(e) => {
                eprintln!("no audio device ({e}); continuing with null sink");
                None
            }
        }
    };
    let out_channels = out.as_ref().map(|o| o.channels).unwrap_or(2);

    // Open every decoder up front so a bad path fails before audio starts.
    let mut sources: Vec<(PassageDecoder, Fade, Fade, QueueEntry)> = Vec::new();
    for c in &cues {
        let d = match PassageDecoder::open(&c.path, c.start_ms, c.end_ms) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("{}: {e}", c.path.display());
                std::process::exit(1);
            }
        };
        let sr = d.sample_rate as f32;
        let fi = Fade { curve: Curve::Exponential, frames: (c.fade_in_s * sr) as u64,
                        fade_in: true };
        let fo = Fade { curve: Curve::Exponential, frames: (c.fade_out_s * sr) as u64,
                        fade_in: false };
        println!("passage: {} [{}ms..{}] {} Hz {} ch",
                 c.path.file_name().unwrap_or_default().to_string_lossy(),
                 c.start_ms,
                 c.end_ms.map(|e| format!("{e}ms")).unwrap_or_else(|| "EOF".into()),
                 d.sample_rate, d.channels);
        let entry = QueueEntry {
            passage_id: sources.len() as i64,
            path: c.path.clone(),
            start_ms: c.start_ms,
            end_ms: c.end_ms.unwrap_or(c.start_ms + 1000 * 60 * 60),
            lead_in_ms: (c.fade_in_s * 1000.0) as u64,
            lead_out_ms: (c.fade_out_s * 1000.0) as u64,
            gain_db: 0.0,
        };
        sources.push((d, fi, fo, entry));
    }

    let block = 2048 * out_channels;
    let mut scratch = vec![0.0f32; block];
    let t0 = Instant::now();
    let mut submitted: u64 = 0;

    // Decoder and its stream stay together; mixing borrows the streams for a
    // block. Owning them here is what lets a passage keep decoding while it is
    // already being mixed -- the crossfade case.
    // frames_total lets admission be driven by playback POSITION rather than
    // buffer level: [XFD-BEH-C1-020] starts B when A reaches its lead-out point.
    // Keying off ring occupancy instead makes the overlap depend on how fast the
    // consumer happens to drain, which silently disappears without a real device.
    struct Live {
        dec: PassageDecoder,
        stream: Stream,
        frames_mixed: u64,
        entry: QueueEntry,
    }
    let mut live: Vec<Live> = Vec::new();
    let mut pending = sources.into_iter();
    let mut next = pending.next();

    loop {
        // Admission uses the shared rule, not a local copy of it: overlap is
        // min(lead_out(A), lead_in(B)) and applies here exactly as it will in
        // the engine [XFD-BEH-C1-020]. A second implementation would be a
        // second thing to get wrong.
        let should_admit = match (&next, live.last()) {
            (Some((_, _, _, nb)), Some(l)) => {
                let played_ms = l.frames_mixed * 1000 / 44_100;
                should_admit(&l.entry, played_ms, nb)
            }
            (Some(_), None) => true,
            _ => false,
        };
        if should_admit {
            if let Some((dec, fade_in, _, entry)) = next.take() {
                let ch = dec.channels;
                if !live.is_empty() {
                    println!("crossfading over {overlap:.1}s");
                }
                live.push(Live {
                    stream: Stream::new(BUFFER_FRAMES * ch, ch, fade_in),
                    dec,
                    frames_mixed: 0,
                    entry,
                });
                next = pending.next();
            }
        }

        // 1. top up every live stream (the fade is applied on the way in)
        for l in live.iter_mut() {
            if l.stream.finished || l.stream.ring.free() < 4096 * l.stream.channels {
                continue;
            }
            match l.dec.next() {
                Ok(Some(chunk)) => {
                    let mut owned = chunk.to_vec();
                    l.stream.push(&mut owned);
                }
                Ok(None) => l.stream.finished = true,
                Err(e) => {
                    eprintln!("decode: {e}");
                    l.stream.finished = true;
                }
            }
        }

        // 2. mix a block and hand it to the output
        let before: Vec<usize> = live.iter().map(|l| l.stream.ring.len()).collect();
        let filled = mix(live.iter_mut().map(|l| &mut l.stream), &mut scratch);
        for (l, was) in live.iter_mut().zip(before) {
            l.frames_mixed += ((was - l.stream.ring.len()) / l.stream.channels.max(1)) as u64;
        }
        if filled > 0 {
            if let Some(o) = &out {
                let mut off = 0;
                while off < filled {
                    let n = o.submit(&scratch[off..filled]);
                    off += n;
                    if n == 0 {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
            submitted += filled as u64;
        }

        live.retain(|l| !l.stream.is_exhausted());
        if live.is_empty() && next.is_none() {
            break;
        }
    }

    // Let the device drain what is still buffered.
    if let Some(o) = &out {
        while o.free() < BUFFER_FRAMES {
            std::thread::sleep(Duration::from_millis(20));
        }
        let (under, locks) = o.diagnostics();
        println!("underrun samples: {under}, lock failures: {locks}");
    }
    println!("submitted {:.1}s of audio in {:.1}s wall",
             submitted as f64 / (44_100.0 * out_channels as f64),
             t0.elapsed().as_secs_f64());
}
