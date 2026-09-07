//! Read-only access to `vaino.db` for playback.
//!
//! Deliberately narrow: the player reads passages and writes play history, and
//! nothing else. Library building belongs to Sampo `[SPEC-SA-100]`, so the
//! queries here are the few the audio path actually needs. A general-purpose
//! DAO would be a second source of truth for the schema `[SPEC008]`.
//!
//! Split along the guard [`Library::open`]'s own doc comment states: [`Library`] is
//! the read-only connection everything on the *reading* path uses --
//! selection, naming, browsing -- and [`PlayerStore`] is the one writable
//! handle, the only place a table gets created. One file used to hold both,
//! [`Library`]'s own methods split across two non-contiguous regions with
//! [`PlayerStore`] sandwiched between them; now each is its own file, so the
//! guard that connection-mode already enforces at runtime is legible at the
//! file level too.

mod library;
mod player_store;

pub use library::*;
pub use player_store::*;

#[derive(Debug)]
pub enum DbError {
    Open(String),
    Query(String),
}

impl DbError {
    /// The message without the "query:" / "open database:" prefix.
    ///
    /// Some of these are refusals meant for a person to read -- "already
    /// applied to the library" -- and reach a browser as the explanation of a
    /// 409. Prefixing that with the name of an internal error variant tells
    /// the reader nothing and makes a considered refusal look like a crash.
    pub fn message(&self) -> &str {
        match self {
            DbError::Open(e) | DbError::Query(e) => e,
        }
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbError::Open(e) => write!(f, "open database: {e}"),
            DbError::Query(e) => write!(f, "query: {e}"),
        }
    }
}

/// How long a connection waits for a writer to finish before giving up.
///
/// Three writers share this file -- the resume row every second, the tag scan
/// at startup, and nothing else -- and WAL allows one at a time. **Without a
/// busy timeout SQLite does not wait at all**: a contended write returns
/// SQLITE_BUSY immediately, and the tag scan's per-file error path would drop
/// that file's row and carry on, leaving a hole nothing would ever revisit.
/// Five seconds is far longer than any write here takes and far shorter than
/// anyone would wait for a stuck one.
pub(crate) const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Attaches `library_path` as `lib`, read-only, when it differs from
/// `db_path`, and returns the schema prefix every catalog query should use
/// from then on `[IMPL-DBSPLIT-025]`.
///
/// The two paths are equal for every installation that hasn't split its
/// database -- `bose`, today's vainopi, every existing test fixture. In
/// that case nothing is attached and this returns `"main"`, so a query
/// written `{lib}.recordings` reads the same table through the same
/// connection it always has, and the one player binary needs no
/// environment-specific build. Only an installation with a genuinely
/// separate `library.db` (vainopi, once split -- `[PI-DB-020]`) ever
/// attaches anything or ever sees `"lib"` come back.
///
/// Equality is the caller's to guarantee, by construction rather than by
/// canonicalizing paths here: the CLI defaults `--library` to a clone of
/// `--db` when the flag is absent, so the two `Path`s are byte-identical
/// in the common case rather than merely referring to the same file on
/// disk. A caller that passes two different spellings of the same path
/// gets a harmless self-attach, not a silent bug -- see the doc comment on
/// the URI mode below for why that's still safe.
///
/// Requires the connection to have been opened with `OpenFlags::SQLITE_OPEN_URI`
/// -- `mode=ro` in the attach URI is SQLite's own documented way to attach
/// a second file read-only, and it only parses as a URI (rather than a
/// literal filename containing a `?`) when the connection has URI filenames
/// enabled. Every one of `Library::open`, `PlayerStore::open`, and
/// `bin/mpd_direct.rs`'s bare `Connection::open` must include that flag
/// once they call this.
pub fn attach_library(
    conn: &rusqlite::Connection,
    db_path: &std::path::Path,
    library_path: &std::path::Path,
) -> Result<&'static str, DbError> {
    if db_path == library_path {
        return Ok("main");
    }
    let uri = format!("file:{}?mode=ro", library_path.display());
    conn.execute("ATTACH DATABASE ?1 AS lib", [uri])
        .map_err(|e| DbError::Open(format!("attach library {}: {e}", library_path.display())))?;
    Ok("lib")
}

#[cfg(test)]
mod attach_library_tests {
    use super::attach_library;
    use rusqlite::{Connection, OpenFlags};
    use std::path::{Path, PathBuf};

    /// Matches the rest of this crate's tests (`player_store.rs`'s own
    /// `vaino-rev-{pid}.db` pattern) rather than introducing a `tempfile`
    /// dependency for one new test module.
    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("vaino-attach-{name}-{}.db", std::process::id()))
    }

    fn open_uri(path: &Path) -> Connection {
        let _ = std::fs::remove_file(path);
        Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap()
    }

    /// The common case today: every installation that hasn't split passes
    /// the same path twice. Nothing should be attached, and `main` must
    /// still resolve -- the whole point is that this costs nothing when
    /// there is no split.
    #[test]
    fn same_path_attaches_nothing_and_returns_main() {
        let path = scratch_path("same");
        let conn = open_uri(&path);
        conn.execute("CREATE TABLE recordings (mbid TEXT PRIMARY KEY)", [])
            .unwrap();

        let alias = attach_library(&conn, &path, &path).unwrap();
        assert_eq!(alias, "main");

        // The whole reason this matters: a query built with the returned
        // alias must actually run, unqualified through `main`.
        conn.execute(&format!("INSERT INTO {alias}.recordings VALUES ('x')"), [])
            .unwrap();
        let _ = std::fs::remove_file(&path);
    }

    /// The split case: two real files, attached read-only, queryable
    /// through the alias this function hands back -- proven against the
    /// actual linked SQLite rather than assumed from documentation.
    #[test]
    fn different_paths_attach_library_read_only_as_lib() {
        let listener_path = scratch_path("listener");
        let library_path = scratch_path("library");

        let lib_conn = open_uri(&library_path);
        lib_conn
            .execute("CREATE TABLE recordings (mbid TEXT PRIMARY KEY)", [])
            .unwrap();
        lib_conn.execute("INSERT INTO recordings VALUES ('abc')", []).unwrap();
        drop(lib_conn);

        let conn = open_uri(&listener_path);
        let alias = attach_library(&conn, &listener_path, &library_path).unwrap();
        assert_eq!(alias, "lib");

        let mbid: String = conn
            .query_row(&format!("SELECT mbid FROM {alias}.recordings"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(mbid, "abc");

        // The safety guard this whole design leans on: a write attempt
        // against the attached schema must fail, not silently succeed.
        let write_err = conn
            .execute(&format!("INSERT INTO {alias}.recordings VALUES ('def')"), [])
            .unwrap_err();
        let msg = write_err.to_string().to_lowercase();
        assert!(
            msg.contains("read-only") || msg.contains("readonly"),
            "expected a read-only failure, got: {write_err}"
        );
        let _ = std::fs::remove_file(&listener_path);
        let _ = std::fs::remove_file(&library_path);
    }

    /// A second connection, attaching the same two files independently --
    /// the shape `Library` and `PlayerStore` actually need, since they are
    /// two separate connections today and stay that way post-split.
    #[test]
    fn two_independent_connections_can_each_attach_the_same_library() {
        let listener_path = scratch_path("listener2");
        let library_path = scratch_path("library2");
        open_uri(&library_path)
            .execute("CREATE TABLE recordings (mbid TEXT PRIMARY KEY)", [])
            .unwrap();

        let conn_a = open_uri(&listener_path);
        let conn_b = Connection::open_with_flags(
            &listener_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
        )
        .unwrap();
        assert_eq!(attach_library(&conn_a, &listener_path, &library_path).unwrap(), "lib");
        assert_eq!(attach_library(&conn_b, &listener_path, &library_path).unwrap(), "lib");

        conn_a.execute("SELECT * FROM lib.recordings", []).unwrap();
        conn_b.execute("SELECT * FROM lib.recordings", []).unwrap();
        let _ = std::fs::remove_file(&listener_path);
        let _ = std::fs::remove_file(&library_path);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Fixtures shared by both [`super::library`]'s and [`super::player_store`]'s
    //! own test modules. Each returns a plain [`Connection`], so building one
    //! here needs no access to either module's private fields -- only the
    //! table constants both already import back via `pub use`.
    use rusqlite::Connection;
    use super::*;

    /// Build the minimum of SPEC008 the player touches, so these tests pin the
    /// column names the queries depend on.
    pub(crate) fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files (file_id INTEGER PRIMARY KEY, audio_md5 TEXT, path TEXT NOT NULL,
                 size_bytes INTEGER, mtime REAL, format TEXT, duration_ms INTEGER,
                 first_seen TEXT, last_seen TEXT);
             CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
                 kind TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                 lead_in_ms INTEGER, lead_out_ms INTEGER, gain_db REAL, boundary_src TEXT);
             CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT,
                 weight REAL DEFAULT 1.0, source TEXT);
             INSERT INTO files VALUES (1,'md5','/m/a.mp3',1,1.0,'mp3',300000,'t','t');
             INSERT INTO passages VALUES (1,1,'album',0,300000,NULL,NULL,NULL,'src');
             INSERT INTO passages VALUES (2,1,'radio',1200,298000,3000,4000,-2.5,'src');
             -- passage 2 is a medley: two recordings, the heavier one wins
             INSERT INTO passage_recordings VALUES (2,'aaaaaaaa-0000-0000-0000-00000000000f',0.3,'s'),
                                                   (2,'aaaaaaaa-0000-0000-0000-000000000001',0.9,'s');
             -- Added after the fact, exactly like the real migration
             -- (`tools/add_fade_columns.py`) adds them to a live database
             -- `[SPEC-SUI-226]` -- so every existing bare `INSERT INTO
             -- passages VALUES (...)` above and in every test built on this
             -- fixture keeps working unmodified, backfilled with the same
             -- default a real ALTER TABLE gives every existing row.
             ALTER TABLE passages ADD COLUMN fade_in_ms INTEGER NOT NULL DEFAULT 20;
             ALTER TABLE passages ADD COLUMN fade_out_ms INTEGER NOT NULL DEFAULT 20;
             ALTER TABLE passages ADD COLUMN fade_in_curve TEXT NOT NULL DEFAULT 'exponential';
             ALTER TABLE passages ADD COLUMN fade_out_curve TEXT NOT NULL DEFAULT 'exponential';",
        )
        .unwrap();
        c
    }

    /// The review fixture: the naming tables the queue joins, plus findings.
    pub(crate) fn reviewable() -> Connection {
        let c = fixture();
        c.execute_batch(
            // NOT NULL exactly as SPEC008 declares them. The looser fixture
            // this replaces is what let a writer that omitted `source` pass
            // every test and then fail against the real library: `INSERT OR
            // IGNORE` turned the violation into nothing happening, and the
            // foreign key failed on the statement after.
            "CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT NOT NULL,
                 length_ms INTEGER, source TEXT NOT NULL);
             CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT NOT NULL,
                 sort_name TEXT, source TEXT NOT NULL);
             CREATE TABLE recording_artists (
                 mbid TEXT NOT NULL REFERENCES recordings(mbid),
                 artist_mbid TEXT NOT NULL REFERENCES artists(mbid),
                 weight REAL NOT NULL DEFAULT 1.0, source TEXT NOT NULL,
                 PRIMARY KEY (mbid, artist_mbid));
             CREATE TABLE release_recordings (release_mbid TEXT, mbid TEXT, position INTEGER,
                 source TEXT, track_length_ms INTEGER, chosen INTEGER DEFAULT 0, disc INTEGER);
             CREATE TABLE releases (mbid TEXT PRIMARY KEY, title TEXT, release_date TEXT,
                 source TEXT, release_group TEXT, status TEXT, primary_type TEXT,
                 secondary_types TEXT, country TEXT, track_count INTEGER);
             -- As SPEC008 declares it, not as the queries here happen to need
             -- it. A fixture missing `played_at` passed for as long as nothing
             -- indexed the column, and then failed the moment the player began
             -- creating the table itself -- the same looseness the note above
             -- is about.
             CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                 played_at INTEGER NOT NULL DEFAULT 0, passage_id INTEGER, mbid TEXT);
             INSERT INTO recordings VALUES ('aaaaaaaa-0000-0000-0000-000000000001','Wrong Song',NULL,'s');
             CREATE TABLE id_checks (passage_id INTEGER PRIMARY KEY, stored_mbid TEXT NOT NULL,
                 verdict TEXT NOT NULL, score REAL, suggested TEXT, checked_at TEXT NOT NULL);
             INSERT INTO id_checks VALUES
                 (2,'aaaaaaaa-0000-0000-0000-000000000001','contradicted',0.97,
                  '[{\"mbid\":\"aaaaaaaa-0000-0000-0000-000000000002\",\
                     \"title\":\"Right Song\",\"artist\":\"A Band\",\"score\":0.97}]','t');",
        )
        .unwrap();
        c.execute_batch(TAG_TABLE).unwrap();
        #[cfg(feature = "sampo-support")]
        ensure_review_table(&c).unwrap();
        #[cfg(feature = "sampo-support")]
        ensure_artist_review_table(&c).unwrap();
        c
    }

    /// The segment-queue fixture `[SPEC024 §7]`: everything `reviewable()`
    /// already sets up (naming tables, `file_tags`) plus `boundary_reviews`
    /// -- created here the same way `PlayerStore::open` creates it for a
    /// running server, since `reviewable()` stops at `ensure_review_table`/
    /// `ensure_artist_review_table` -- and `ingest_decisions` `[SPEC008
    /// §7]`, which neither fixture includes since the ordinary playback and
    /// identification paths never touch it.
    pub(crate) fn segmentable() -> Connection {
        let c = reviewable();
        #[cfg(feature = "sampo-support")]
        ensure_boundary_review_table(&c).unwrap();
        c.execute_batch(
            "CREATE TABLE ingest_decisions (decision_id INTEGER PRIMARY KEY, audio_md5 TEXT NOT NULL,
                 stage TEXT NOT NULL, outcome TEXT NOT NULL, confidence REAL, detail BLOB,
                 decided_at TEXT NOT NULL);",
        )
        .unwrap();
        c
    }

    /// The naming tables `play_history` reads, filled with one recording that
    /// has both an artist and a chosen release -- enough to exercise every
    /// column the page shows.
    pub(crate) fn historyable() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE recordings (mbid TEXT PRIMARY KEY, title TEXT NOT NULL,
                 length_ms INTEGER, source TEXT NOT NULL);
             CREATE TABLE artists (mbid TEXT PRIMARY KEY, name TEXT NOT NULL,
                 sort_name TEXT, source TEXT NOT NULL);
             CREATE TABLE recording_artists (mbid TEXT, artist_mbid TEXT,
                 weight REAL DEFAULT 1.0, source TEXT);
             CREATE TABLE releases (mbid TEXT PRIMARY KEY, title TEXT, release_date TEXT,
                 source TEXT);
             CREATE TABLE release_recordings (release_mbid TEXT, mbid TEXT, position INTEGER,
                 source TEXT, chosen INTEGER DEFAULT 0);
             INSERT INTO recordings VALUES ('aaaaaaaa-0000-0000-0000-000000000001','A Song',NULL,'s');
             INSERT INTO artists VALUES ('bbbbbbbb-0000-0000-0000-000000000001','A Band',NULL,'s');
             INSERT INTO recording_artists VALUES
                 ('aaaaaaaa-0000-0000-0000-000000000001','bbbbbbbb-0000-0000-0000-000000000001',1.0,'s');
             INSERT INTO releases VALUES ('cccccccc-0000-0000-0000-000000000001','An Album',NULL,'s');
             INSERT INTO release_recordings VALUES
                 ('cccccccc-0000-0000-0000-000000000001','aaaaaaaa-0000-0000-0000-000000000001',1,'s',1);",
        )
        .unwrap();
        c.execute_batch(PLAY_TABLE).unwrap();
        c.execute_batch(REJECTION_TABLE).unwrap();
        c.execute_batch(FLAGS_TABLE).unwrap();
        c.execute_batch(PREFERENCES_TABLE).unwrap();
        c
    }

}
