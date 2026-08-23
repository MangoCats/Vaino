//! Agreements between modules that no single module can keep.
//!
//! Every module here is well covered by its own unit tests. What those cannot
//! reach is the place two modules have to **agree** — one writes a file and
//! another addresses it, one saves a setting and another reads it back — where
//! each side is correct on its own terms and the pair is wrong.
//!
//! The review that prompted these found the cue numbering held together by a
//! comment reading *"the two must agree, and this is the place they do"*, with
//! nothing failing if they stopped.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// A temporary directory that cleans up after itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(what: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "vaino_it_{what}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn join(&self, s: &str) -> PathBuf {
        self.0.join(s)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The tables the folder-writing modules read, and nothing else.
///
/// A real library is a hundred-odd tables; these functions touch eight of them.
/// Building only those keeps the fixture readable and makes it obvious when one
/// of them starts reading something new.
fn library(dir: &Path) -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT, duration_ms INTEGER);
         CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER,
                                kind TEXT, start_ms INTEGER, end_ms INTEGER);
         CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT);
         CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT);
         CREATE TABLE recording_artists (mbid TEXT, artist_mbid TEXT, weight REAL);
         CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT);
         CREATE TABLE file_tags (file_id INTEGER PRIMARY KEY, title TEXT,
                                 artist TEXT, album TEXT);
         CREATE TABLE lyrics (mbid TEXT PRIMARY KEY, text TEXT);",
    )
    .unwrap();

    // One capture: three songs inside a single file, by three different people,
    // which is the shape everything interesting here is about.
    let capture = dir.join("Live.mp3");
    c.execute(
        "INSERT INTO files VALUES (1, ?1, 900000)",
        rusqlite::params![capture.to_string_lossy()],
    )
    .unwrap();
    c.execute("INSERT INTO file_tags VALUES (1, NULL, NULL, 'The Concert')", []).unwrap();

    for (pid, start, end, title, artist) in [
        (101, 0, 300_000, "Opening", "First Band"),
        (102, 300_000, 600_000, "The Middle One", "Second Band"),
        (103, 600_000, 900_000, "Closing", "Third Band"),
    ] {
        c.execute(
            "INSERT INTO passages VALUES (?1, 1, 'radio', ?2, ?3)",
            rusqlite::params![pid, start, end],
        )
        .unwrap();
        let mbid = format!("mb-{pid}");
        c.execute(
            "INSERT INTO passage_recordings VALUES (?1, ?2)",
            rusqlite::params![pid, mbid],
        )
        .unwrap();
        c.execute(
            "INSERT INTO recordings VALUES (?1, ?2)",
            rusqlite::params![mbid, title],
        )
        .unwrap();
        let amb = format!("art-{pid}");
        c.execute(
            "INSERT INTO recording_artists VALUES (?1, ?2, 1.0)",
            rusqlite::params![mbid, amb],
        )
        .unwrap();
        c.execute("INSERT INTO artists VALUES (?1, ?2)", rusqlite::params![amb, artist]).unwrap();
        c.execute(
            "INSERT INTO lyrics VALUES (?1, ?2)",
            rusqlite::params![mbid, format!("words for {title}")],
        )
        .unwrap();
    }
    std::fs::write(&capture, b"not really audio").unwrap();
    c
}

/// `TRACK nn` → the title under it, read back out of a written sheet.
fn tracks_in(sheet: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut n = 0;
    for line in sheet.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("TRACK ") {
            n = rest.split_whitespace().next().unwrap().parse().unwrap();
        } else if let Some(rest) = t.strip_prefix("TITLE \"") {
            if n > 0 {
                out.push((n, rest.trim_end_matches('"').to_string()));
            }
        }
    }
    out
}

/// **The cue sheet's track numbers and the URIs MPD is given must agree.**
///
/// `cue::generate` numbers tracks by `start_ms`; `mpd_backend::cue_uris` builds
/// `…/trackNNNN` from the same ordering, in a different module, from a different
/// query. Nothing but this test fails if one of them changes its `ORDER BY`, and
/// the symptom would be silent: every song in every capture named as its
/// neighbour.
#[cfg(feature = "mpd")]
#[test]
fn the_cue_sheet_and_the_uris_number_tracks_the_same_way() {
    let dir = Scratch::new("cue");
    let conn = library(dir.0.as_path());

    let report = vaino_player::cue::generate(&conn, false).expect("a sheet is written");
    assert_eq!(report.written, 1, "one capture, one sheet");

    let sheet = std::fs::read_to_string(dir.join("Live.cue")).expect("beside the audio");
    let tracks = tracks_in(&sheet);
    assert_eq!(tracks.len(), 3, "one track per passage");

    let root = dir.0.to_string_lossy().replace('\\', "/");
    let uris = vaino_player::mpd_backend::cue_uris(&conn, &root).expect("uris");

    // The passages in the order the sheet was written, paired with the title
    // that sheet gives each one.
    for (passage, expected_title) in [(101, "Opening"), (102, "The Middle One"), (103, "Closing")] {
        let uri = uris.get(&passage).unwrap_or_else(|| panic!("no uri for passage {passage}"));
        let n: usize = uri
            .rsplit("/track")
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("unreadable track number in {uri}"));
        let (_, titled) = tracks
            .iter()
            .find(|(num, _)| *num == n)
            .unwrap_or_else(|| panic!("{uri} points at track {n}, which the sheet has not got"));
        assert_eq!(
            titled, expected_title,
            "passage {passage} is addressed as track {n}, which the sheet calls {titled:?}"
        );
    }
}

/// A capture whose passages are inserted out of order is still numbered by
/// **time**, on both sides — the ordering is `start_ms`, not insertion.
#[cfg(feature = "mpd")]
#[test]
fn the_numbering_follows_the_music_not_the_database() {
    let dir = Scratch::new("order");
    let conn = library(dir.0.as_path());
    // A fourth passage, inserted last, belonging second.
    conn.execute("INSERT INTO passages VALUES (104, 1, 'radio', 150000, 300000)", []).unwrap();
    conn.execute("INSERT INTO passage_recordings VALUES (104, 'mb-104')", []).unwrap();
    conn.execute("INSERT INTO recordings VALUES ('mb-104', 'Late Insert')", []).unwrap();

    vaino_player::cue::generate(&conn, false).unwrap();
    let sheet = std::fs::read_to_string(dir.join("Live.cue")).unwrap();
    let tracks = tracks_in(&sheet);

    assert_eq!(tracks[1].1, "Late Insert", "second by time is track 2 in the sheet");

    let root = dir.0.to_string_lossy().replace('\\', "/");
    let uris = vaino_player::mpd_backend::cue_uris(&conn, &root).unwrap();
    assert!(uris[&104].ends_with("/track0002"), "and track 2 in the URI: {}", uris[&104]);
}

/// **Running a folder-writer twice must touch nothing the second time.**
///
/// Each module asserts this about itself with a mock library; this asserts it
/// about all of them against one real directory, in the order a listener would
/// actually tick the boxes.
#[test]
fn every_folder_writer_is_idempotent() {
    let dir = Scratch::new("idem");
    let conn = library(dir.0.as_path());

    let first = vaino_player::cue::generate(&conn, false).unwrap();
    assert_eq!(first.written, 1);
    let again = vaino_player::cue::generate(&conn, false).unwrap();
    assert_eq!((again.written, again.unchanged), (0, 1), "cue sheets");

    // The sidecar skips this capture on purpose, so give it a file of its own.
    let solo = dir.join("Alone.mp3");
    conn.execute(
        "INSERT INTO files VALUES (2, ?1, 200000)",
        rusqlite::params![solo.to_string_lossy()],
    )
    .unwrap();
    conn.execute("INSERT INTO passages VALUES (201, 2, 'radio', 0, 200000)", []).unwrap();
    conn.execute("INSERT INTO passage_recordings VALUES (201, 'mb-201')", []).unwrap();
    conn.execute("INSERT INTO lyrics VALUES ('mb-201', 'the only words')", []).unwrap();
    std::fs::write(&solo, b"not really audio").unwrap();

    let first = vaino_player::lyrics_sidecar::generate(&conn, false).unwrap();
    assert_eq!(first.written, 1, "one single-passage file has words");
    let again = vaino_player::lyrics_sidecar::generate(&conn, false).unwrap();
    assert_eq!((again.written, again.unchanged), (0, 1), "lyrics sidecars");
}

/// **A file Vaino did not write is never replaced**, by any of them.
///
/// The rule every folder-writer claims. Asserted here against a real file on
/// disk rather than against each module's own idea of one.
#[test]
fn a_file_somebody_else_wrote_is_left_alone() {
    let dir = Scratch::new("keep");
    let conn = library(dir.0.as_path());

    let mine = "REM someone else's sheet\nFILE \"Live.mp3\" MP3\n";
    std::fs::write(dir.join("Live.cue"), mine).unwrap();

    let rep = vaino_player::cue::generate(&conn, false).unwrap();

    assert_eq!(rep.written, 0, "nothing written over it");
    assert_eq!(
        std::fs::read_to_string(dir.join("Live.cue")).unwrap(),
        mine,
        "and it is byte for byte what it was"
    );
}

/// **Settings survive a close and a reopen**, through the real store.
///
/// The unit test proves the key list agrees with itself. This proves the store
/// actually persists it: a different process, a different connection, the same
/// values.
#[test]
fn settings_survive_a_reopen() {
    let dir = Scratch::new("settings");
    let db = dir.join("player.db");

    let want = vaino_player::db::Settings {
        volume: 0.42,
        skip_fade_ms: 1_111,
        skip_lead_ms: 222,
        resume_save_ms: 33_000,
        skip_suppress_h: 44,
        dequeue_suppress_h: 5,
        queue_depth: 6,
        sample_interval_ms: 7_000,
        cue_sheets: true,
        covers: false,
        lyrics_cache: true,
        lyrics_sidecar: false,
    };

    {
        let store = vaino_player::db::PlayerStore::open(&db).expect("a new store");
        assert!(store.load_settings().is_none(), "nothing chosen yet");
        store.save_settings(&want).expect("saved");
    }

    let store = vaino_player::db::PlayerStore::open(&db).expect("reopened");
    assert_eq!(store.load_settings().expect("saved settings are there"), want);
}

/// A database written when settings were columns still opens, and keeps them.
///
/// The migration runs once on open and is invisible afterwards. This is the
/// shipping question: the appliance's library is an old one.
#[test]
fn settings_written_as_columns_are_carried_over() {
    let dir = Scratch::new("migrate");
    let db = dir.join("old.db");
    {
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE player_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 passage_id INTEGER, position_ms INTEGER NOT NULL DEFAULT 0,
                 playing INTEGER NOT NULL DEFAULT 0, volume REAL NOT NULL DEFAULT 1.0,
                 updated_at TEXT NOT NULL,
                 skip_suppress_h INTEGER, cue_sheets INTEGER);
             INSERT INTO player_state
                 (id, position_ms, playing, volume, updated_at, skip_suppress_h, cue_sheets)
                 VALUES (1, 9999, 1, 0.25, datetime('now'), 72, 1);",
        )
        .unwrap();
    }

    let store = vaino_player::db::PlayerStore::open(&db).expect("an old library opens");
    let got = store.load_settings().expect("its settings come over");

    assert!((got.volume - 0.25).abs() < 1e-6, "a REAL column, not read as text");
    assert_eq!(got.skip_suppress_h, 72, "the listener's own window");
    assert!(got.cue_sheets, "and their own choice about writing files");
    // Everything the old database never had falls back to its default, not zero.
    assert_eq!(got.queue_depth, vaino_player::db::Settings::default().queue_depth);
    assert_eq!(store.load().unwrap(), Some((None, 9999, true)), "and the resume point");
}
