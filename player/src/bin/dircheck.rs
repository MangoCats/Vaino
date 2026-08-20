//! What it costs to rebuild the Program Director in place `[SPEC009]`.
//!
//! `Director::load` runs once inside `Session::open` and nothing reloads it, so
//! imported music is browsable at once and unselectable until the player
//! restarts `[IMPL-SUI-070]`. Rebuilding it live instead is attractive because
//! **the Director is off the audio path entirely** — it chooses what plays next
//! and never touches decode, mix or output, so a rebuild cannot glitch a note.
//!
//! Whether it is affordable turns on two numbers, and this measures them rather
//! than reasoning about them — the same shape as `memcheck` for `[REQ-AUD-110]`:
//!
//!   1. **How long** a load takes, against the queue depth that would cover it.
//!   2. **Peak memory while two exist at once**, which is what a build-then-swap
//!      costs on a 512 MB appliance holding to ≤150 MB `[GDE-ARC-050]`.
//!
//! Usage:  dircheck <vaino.db>

use std::path::PathBuf;
use std::time::Instant;

use vaino_player::{db::Library, peak_rss_bytes};

fn mb(b: u64) -> f64 {
    b as f64 / 1_048_576.0
}

fn rss() -> u64 {
    peak_rss_bytes().unwrap_or(0)
}

fn main() {
    let Some(db) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: dircheck <vaino.db>");
        std::process::exit(2);
    };
    let lib = match Library::open(&db) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot open {}: {e:?}", db.display());
            std::process::exit(1);
        }
    };
    let radio = lib.count_radio().unwrap_or(0);
    let base = rss();
    println!("library: {radio} radio passages");
    println!("baseline peak RSS: {:.1} MB\n", mb(base));

    // One, cold.
    let t0 = Instant::now();
    let first = match lib.director() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("director load failed: {e:?}");
            std::process::exit(1);
        }
    };
    let cold = t0.elapsed();
    let after_one = rss();
    println!("first load     {:>8.0} ms   peak RSS {:>7.1} MB  (+{:.1})",
             cold.as_secs_f64() * 1000.0, mb(after_one), mb(after_one - base));

    // Hold several at once. Peak RSS only ever rises, so the FIRST load
    // measures the Director plus every one-time cost behind it -- SQLite's page
    // cache, query scratch, the allocator's first reach for pages. The MARGINAL
    // cost of one more is what a build-then-swap actually pays, and only
    // holding two at a time reveals it.
    let mut held = vec![first];
    let mut prev = after_one;
    let mut warm = std::time::Duration::ZERO;
    for n in 2..=4 {
        let t1 = Instant::now();
        held.push(lib.director().expect("another load"));
        let took = t1.elapsed();
        if n == 2 {
            warm = took;
        }
        let now = rss();
        println!("holding {n}      {:>8.0} ms   peak RSS {:>7.1} MB  (+{:.1} for this one)",
                 took.as_secs_f64() * 1000.0, mb(now), mb(now.saturating_sub(prev)));
        prev = now;
    }
    let marginal = prev.saturating_sub(after_one) / 3;
    drop(held);

    println!();
    println!("first load     ~{:.1} MB   Director + SQLite cache + query scratch",
             mb(after_one.saturating_sub(base)));
    println!("each further   ~{:.1} MB   <- what a live swap transiently costs", mb(marginal));
    println!();
    // The queue is the buffer. A rebuild is affordable when the audio already
    // queued outlasts it by a wide margin -- the cost of being wrong is a late
    // refill, never a gap, because selection is not the audio path.
    let secs = cold.as_secs_f64().max(warm.as_secs_f64());
    println!("a {:.2} s load sits inside a 180 s queue {:.0}x over -- inside one track.",
             secs, 180.0 / secs.max(0.001));
    if mb(marginal) > 30.0 {
        println!("NOTE: {:.0} MB marginal against the 150 MB budget [GDE-ARC-050] argues \
                  for drop-then-load rather than build-then-swap.", mb(marginal));
    }
}

#[cfg(test)]
mod tests {
    /// A live rebuild builds the new Director on its own thread, off the
    /// selection path, and hands it over when it is ready. That requires it to
    /// be `Send`; if it ever gains a `Connection` or an `Rc` this fails to
    /// compile, which is the point of asserting it here rather than finding out
    /// in the middle of writing the reload.
    #[test]
    fn director_can_cross_a_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<vaino_player::director::library::Director>();
    }
}
