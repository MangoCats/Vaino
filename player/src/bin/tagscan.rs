//! Read every file's own tags into the library, once.
//!
//! Browsing by album has no other source: MusicBrainz Release data is the right
//! answer and Vaino has none of it, so the file's own ALBUM tag is what there
//! is `[REQ-VIS-180]`. Reading them means opening and probing every file, which
//! is far too slow to do on demand for a whole library -- so it happens here,
//! deliberately, and the answers are kept.
//!
//! Safe to re-run: rows are upserted, so a rescan after adding music costs only
//! the files themselves.
//!
//!   tagscan <library.db>

use vaino_player::db::Library;

fn main() {
    let Some(db) = std::env::args().nth(1) else {
        eprintln!("usage: tagscan <library.db>");
        std::process::exit(2);
    };
    let lib = match Library::open_writable(std::path::Path::new(&db)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("open {db}: {e}");
            std::process::exit(1);
        }
    };
    if let Err(e) = lib.ensure_tag_table() {
        eprintln!("create file_tags: {e}");
        std::process::exit(1);
    }
    let files = match lib.all_files() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("list files: {e}");
            std::process::exit(1);
        }
    };

    let started = std::time::Instant::now();
    let (mut tagged, mut art, mut failed) = (0u32, 0u32, 0u32);
    for (i, (file_id, path)) in files.iter().enumerate() {
        let t = vaino_player::tags::read(path);
        let has_art = vaino_player::tags::artwork(path).is_some();
        if t.is_empty() && !has_art {
            failed += 1;
        } else {
            tagged += 1;
        }
        if has_art {
            art += 1;
        }
        if let Err(e) = lib.put_tags(*file_id, &t, has_art) {
            eprintln!("store {file_id}: {e}");
        }
        // Progress, because this walks the whole library and a silent minute
        // looks like a hang [REQ-VIS-140].
        if i % 250 == 249 || i + 1 == files.len() {
            println!(
                "  {}/{} files, {tagged} tagged, {art} with art, {failed} with neither ({:.0}s)",
                i + 1,
                files.len(),
                started.elapsed().as_secs_f32()
            );
        }
    }
    println!(
        "scanned {} files in {:.1}s: {tagged} tagged, {art} with cover art, {failed} with neither",
        files.len(),
        started.elapsed().as_secs_f32()
    );
}
