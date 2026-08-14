//! Read every file's own tags into the library.
//!
//! The player does this by itself in the background at startup, incrementally
//! `[REQ-VIS-180]`. This tool exists for the cases that are not a startup: a
//! library being prepared before it is ever played, or one whose files have
//! been re-tagged and need reading again.
//!
//!   tagscan <library.db>          scan whatever has never been scanned
//!   tagscan <library.db> --all    scan everything again

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(db) = args.first() else {
        eprintln!("usage: tagscan <library.db> [--all]");
        std::process::exit(2);
    };
    let db = std::path::Path::new(db);

    if args.iter().any(|a| a == "--all") {
        // Forget what is known, so every file is read afresh.
        match vaino_player::db::Library::open_writable(db) {
            Ok(lib) => {
                if let Err(e) = lib.forget_tags() {
                    eprintln!("clear tags: {e}");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("open {}: {e}", db.display());
                std::process::exit(1);
            }
        }
    }

    match vaino_player::tags::backfill(db, true) {
        Ok((0, _)) => println!("nothing to scan: every file already has tags"),
        Ok((n, art)) => println!("{n} file(s) scanned, {art} with cover art"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
