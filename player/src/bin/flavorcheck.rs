//! What flavor distance actually does on a real library.
//!
//! Not a test — a measurement. It reports the schema it found, the distance
//! distribution, and nearest/farthest neighbours for a few recordings, because
//! `[SPEC-FD-070]` is explicit that the retrieval tests validate *robustness*
//! and not perceptual similarity. Reading actual neighbour lists is the only
//! check available on that until `[SPEC-FD-080]` can run.
//!
//! Usage:  flavorcheck <vaino.db> [samples]

use std::path::PathBuf;
use std::time::Instant;

use vaino_player::director::flavor::FlavorIndex;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: flavorcheck <vaino.db> [samples]");
        std::process::exit(2);
    }
    let db = PathBuf::from(&args[0]);
    let samples: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);

    let conn = match rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("open: {e}");
            std::process::exit(1);
        }
    };

    let t0 = Instant::now();
    let idx = match FlavorIndex::load(&conn) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("loaded {} subjects in {:.2}s", idx.len(), t0.elapsed().as_secs_f64());
    println!("malformed characteristic instances: {} [SPEC-FD-100]", idx.malformed);
    println!("\ncharacteristics in use:");
    println!("  {:<24} {:>8} {:>8}", "name", "beta", "weight");
    for c in 0..idx.schema.characteristic_count() {
        println!(
            "  {:<24} {:>8.4} {:>8.4}",
            idx.schema.name(c),
            idx.schema.beta(c),
            idx.schema.weight(c)
        );
    }

    // Titles, so a neighbour list can be read rather than merely counted.
    let mut title = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT r.mbid, r.title, a.name FROM recordings r \
         LEFT JOIN recording_artists ra ON ra.mbid = r.mbid \
         LEFT JOIN artists a ON a.mbid = ra.artist_mbid",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        }) {
            for (m, t, a) in rows.flatten() {
                title.insert(m, format!("{a} — {t}"));
            }
        }
    }

    let ids: Vec<String> = {
        let mut v: Vec<String> = idx.subjects().cloned().collect();
        v.sort();
        v
    };
    if ids.len() < 2 {
        println!("\nnot enough flavor to compare");
        return;
    }

    // Distance distribution over a strided sample of pairs.
    let stride = (ids.len() / 7).max(1);
    let mut ds: Vec<f64> = Vec::new();
    let mut incomparable = 0usize;
    let t1 = Instant::now();
    for i in 0..ids.len().min(samples) {
        let j = (i * stride + 1) % ids.len();
        if i == j {
            continue;
        }
        match idx.distance(&ids[i], &ids[j]) {
            Some(d) => ds.push(d),
            None => incomparable += 1,
        }
    }
    ds.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| ds[((ds.len() - 1) as f64 * p) as usize];
    println!(
        "\n{} pairs in {:.0} ms | incomparable {}",
        ds.len(),
        t1.elapsed().as_secs_f64() * 1000.0,
        incomparable
    );
    if !ds.is_empty() {
        println!(
            "distance  min {:.3}  p05 {:.3}  median {:.3}  p95 {:.3}  max {:.3}",
            ds[0],
            pct(0.05),
            pct(0.50),
            pct(0.95),
            ds[ds.len() - 1]
        );
    }

    // Nearest and farthest for a handful of recordings -- the only perceptual
    // check available before [SPEC-FD-080].
    let t2 = Instant::now();
    let mut compared = 0usize;
    for probe in ids.iter().step_by(ids.len() / 3).take(3) {
        let mut scored: Vec<(f64, &String)> = ids
            .iter()
            .filter(|o| *o != probe)
            .filter_map(|o| idx.distance(probe, o).map(|d| (d, o)))
            .collect();
        compared += scored.len();
        scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let name = |m: &String| title.get(m).cloned().unwrap_or_else(|| m.clone());
        println!("\n{}", name(probe));
        for (d, m) in scored.iter().take(3) {
            println!("   near  {d:.3}  {}", name(m));
        }
        for (d, m) in scored.iter().rev().take(1) {
            println!("   far   {d:.3}  {}", name(m));
        }
    }
    println!(
        "\n{compared} distances in {:.0} ms ({:.1} us each)",
        t2.elapsed().as_secs_f64() * 1000.0,
        t2.elapsed().as_secs_f64() * 1e6 / compared.max(1) as f64
    );
}
