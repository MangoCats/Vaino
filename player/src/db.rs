//! Read-only access to `vaino.db` for playback.
//!
//! Deliberately narrow: the player reads passages and writes play history, and
//! nothing else. Library building belongs to Sampo `[SPEC-SA-100]`, so the
//! queries here are the few the audio path actually needs. A general-purpose
//! DAO would be a second source of truth for the schema `[SPEC008]`.
//!
//! Everything returns [`QueueEntry`] — the type the scheduler already speaks —
//! so the database layer adds no vocabulary of its own.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

use crate::queue::{Naming, QueueEntry};

pub struct Library {
    conn: Connection,
}

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

/// The one place the passage/file join is written. Every loader below selects
/// these columns in this order, so `row_to_entry` can stay a single function.
/// Kept as columns and source separately so the Program Director can select
/// these columns *plus its own* and still map the row with [`row_to_entry`].
/// A second hand-written copy of this join would be a second place to get the
/// fade columns wrong.
/// Fill in what a passage is called, and how often it has been heard
/// `[REQ-VIS-170]`.
///
/// Deliberately NOT part of `COLS`. The Director loads the whole radio pool
/// through those columns -- 8,078 rows -- and five correlated subqueries there
/// would be five subqueries eight thousand times, to answer a question about
/// weighting that names have nothing to do with. Display metadata is fetched
/// for the dozen passages actually on screen, where it costs under a
/// millisecond each.
const DESCRIBE: &str = "    SELECT (SELECT r.title FROM recordings r WHERE r.mbid = m.mbid),            (SELECT a.name FROM recording_artists ra               JOIN artists a ON a.mbid = ra.artist_mbid              WHERE ra.mbid = m.mbid ORDER BY ra.weight DESC, a.name LIMIT 1),            (SELECT rel.title FROM release_recordings rr               JOIN releases rel ON rel.mbid = rr.release_mbid              WHERE rr.mbid = m.mbid ORDER BY rr.chosen DESC, rel.release_date, rel.title LIMIT 1),            (SELECT COUNT(*) FROM listener_play_history h WHERE h.mbid = m.mbid),            (SELECT MAX(h.played_at) FROM listener_play_history h WHERE h.mbid = m.mbid)       FROM (SELECT ?1 AS mbid) m";

/// The tag index, defined once `[REQ-VIS-180]`.
///
/// Created by whoever holds a writable handle: `tagscan` when it fills it, and
/// the player at startup when it does not exist at all. **Browsing joins this
/// table, so a missing one is not an empty result but a failed query** -- the
/// first version shipped without this and every browse page came up blank on a
/// library that had never been scanned.
/// How long a connection waits for a writer to finish before giving up.
///
/// Three writers share this file -- the resume row every second, the tag scan
/// at startup, and nothing else -- and WAL allows one at a time. **Without a
/// busy timeout SQLite does not wait at all**: a contended write returns
/// SQLITE_BUSY immediately, and the tag scan's per-file error path would drop
/// that file's row and carry on, leaving a hole nothing would ever revisit.
/// Five seconds is far longer than any write here takes and far shorter than
/// anyone would wait for a stuck one.
const BUSY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

pub(crate) const TAG_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS file_tags (
        file_id INTEGER PRIMARY KEY,
        title TEXT, artist TEXT, album TEXT,
        track_no INTEGER, disc_no INTEGER,
        has_art INTEGER NOT NULL DEFAULT 0,
        scanned_at INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS idx_file_tags_album ON file_tags(album);
    CREATE INDEX IF NOT EXISTS idx_file_tags_artist ON file_tags(artist);";

/// Cover art fetched from the Cover Art Archive `[REQ-VIS-170]`.
///
/// Keyed by **release**, not by folder. A directory can hold more than one
/// album -- which is exactly the case on the DAO rips that make up most of
/// this library's missing art -- and `folder.jpg` cannot tell them apart.
///
/// Written only by `tools/fetch_cover_art.py`; the player reads it. It is a
/// permanent cache in the sense `[SPEC-SA-020]` gives the lowlevel features:
/// derived from an external service, expensive to obtain, and pointless to
/// re-derive on every rebuild.
///
/// Front and back, because MuLibPlay carried both and its interface showed
/// both side by side. Nothing else is stored: MuLibPlay's back covers are
/// 35.6 MB of its 80.5 MB of art, and a third image nobody displays would be
/// the same bargain again.
/// What the listener declined, and how `[SPEC-PLAY-050]`, `[SPEC-PLAY-055]`.
///
/// Created by the player rather than left to Sampo's schema pass, for the same
/// reason as the tables below it: the player is the one **writing** here, on
/// every rejection, and an existing library that predates this feature would
/// otherwise fail every write. Those writes are best-effort by design, so the
/// failure would be a log line and a suppression that silently never happened.
/// Where plays are written `[SPEC-PLAY-010]`.
///
/// Created by the player for exactly the reason `REJECTION_TABLE` is: the
/// player is what **writes** here, on every play, and those writes are
/// best-effort by design. A library that reached this build without Sampo
/// having run its schema would have failed every one of them as a log line
/// nobody reads — losing the only irreplaceable data in the system
/// `[SPEC-DF-090]` while appearing to work.
pub(crate) const PLAY_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS listener_play_history (
        play_id     INTEGER PRIMARY KEY,
        played_at   INTEGER NOT NULL,
        passage_id  INTEGER,
        mbid        TEXT,
        -- How much of the passage was heard, and how long it was, in ms
        -- `[REQ-VIS-250]`. Absent on rows written before this column existed --
        -- a NULL history page reads as \"unknown\", never as 0%.
        heard_ms    INTEGER,
        span_ms     INTEGER);
    CREATE INDEX IF NOT EXISTS listener_play_time ON listener_play_history(played_at);
    CREATE INDEX IF NOT EXISTS listener_play_mbid ON listener_play_history(mbid);";

pub(crate) const REJECTION_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS listener_rejections (
        rejection_id INTEGER PRIMARY KEY,
        rejected_at  INTEGER NOT NULL,
        kind         TEXT NOT NULL,
        passage_id   INTEGER,
        mbid         TEXT,
        -- Only ever set for 'skip' -- a 'dequeue' never sounded, so it has no
        -- percentage to report `[REQ-VIS-250]`.
        heard_ms     INTEGER,
        span_ms      INTEGER);
    CREATE INDEX IF NOT EXISTS listener_reject_mbid
        ON listener_rejections(mbid, kind);";

/// "Please look at this" from the listener's own chair `[REQ-VIS-265]`, set
/// and cleared from the play-history page at any time -- a plain flag, not a
/// judgement, so it carries no verdict of its own. Keyed the way `flavor`
/// already is: a recording when the play had one, a passage when it did not,
/// because a track worth flagging is often exactly the one with no MBID yet.
pub(crate) const FLAGS_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS listener_flags (
        subject_kind TEXT NOT NULL CHECK (subject_kind IN ('recording','passage')),
        subject_id   TEXT NOT NULL,
        flagged_at   TEXT NOT NULL,
        origin       TEXT,
        PRIMARY KEY (subject_kind, subject_id)) WITHOUT ROWID;";

/// Bring a `listener_flags` predating `origin` up to date `[SPEC-DF-107]`,
/// the same reason `ensure_history_columns` exists for the two tables beside
/// it: `CREATE TABLE IF NOT EXISTS` above is a no-op on a library that
/// already has the table. Vaino itself never writes this column -- only
/// `tools/import_flags.py`, landing a flag pulled from elsewhere, does -- but
/// the column has to already exist there for that write to have somewhere to
/// go, on every appliance, not just ones a sync tool happens to reach first.
fn ensure_flags_columns(conn: &Connection) {
    let _ = conn.execute("ALTER TABLE listener_flags ADD COLUMN origin TEXT", []);
}

/// Bring an existing `listener_play_history` / `listener_rejections` up to the
/// column set above `[REQ-VIS-250]`.
///
/// A library built before this feature has both tables already, so the
/// `CREATE TABLE IF NOT EXISTS` above is a no-op on it -- the same shape of
/// gap `ensure_review_table` and `ensure_tag_table` close for their own
/// tables. Already-present columns fail here, which is the expected path on
/// every start after the first.
fn ensure_history_columns(conn: &Connection) {
    for (table, column) in [
        ("listener_play_history", "heard_ms INTEGER"),
        ("listener_play_history", "span_ms INTEGER"),
        ("listener_rejections", "heard_ms INTEGER"),
        ("listener_rejections", "span_ms INTEGER"),
    ] {
        let _ = conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column}"), []);
    }
}

pub(crate) const ART_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS cover_art (
        release_mbid TEXT PRIMARY KEY,
        front        BLOB,
        back         BLOB,
        source       TEXT NOT NULL,
        fetched_at   TEXT NOT NULL);";

/// Judgements a person has made about a questionable recording id
/// `[REQ-LIB-165]`.
///
/// Separate from `passage_recordings`, and the player never touches that table.
/// Three reasons, and the last is the one that matters:
///
/// * the read-only guard on the library survives -- a bug here still cannot
///   corrupt what Sampo built;
/// * the decision is reversible, because the evidence it overrode is still
///   there to compare against;
/// * a reassignment changes what a passage *is*, and play history is keyed by
///   recording. Rewriting the id in place would silently re-attribute every
///   past play of it. That is a migration, and migrations belong to Sampo.
///
/// So this is a decision log the player owns and honours, and
/// `tools/apply_reviews.py` is what folds accepted decisions into the library
/// proper -- deliberately a separate, deliberate step.
///
/// Gated behind `sampo-support` along with everything else on this page
/// `[SPEC-SUI-190]`: an appliance that never runs Sampo has nothing to review.
#[cfg(feature = "sampo-support")]
pub(crate) const REVIEW_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS id_reviews (
        passage_id  INTEGER PRIMARY KEY,
        -- 'kept': the stored id is right despite the fingerprint.
        -- 'reassigned': chosen_mbid is right.
        -- 'deferred': looked at, not decided; stays out of the queue.
        decision    TEXT NOT NULL,
        chosen_mbid TEXT,
        decided_at  TEXT NOT NULL);";

/// Columns added to `id_reviews` after the first version of it shipped.
///
/// `previous_mbid` is what makes an applied decision reversible: once
/// `apply_reviews` has rewritten `passage_recordings`, the old id exists
/// nowhere else, and an undo with nothing to restore is not an undo.
///
/// `applied_at` is the difference between a judgement that can simply be
/// withdrawn and one that has already changed the library. The page must not
/// offer the same button for both.
/// `origin` is added here and to the other two review tables together
/// `[SPEC-DF-104]`: `NULL` for a decision made on this machine, else the
/// hostname it was synced in from -- so a decision's own history survives
/// however many installations it has already crossed.
#[cfg(feature = "sampo-support")]
pub(crate) const REVIEW_COLUMNS: [&str; 4] = [
    "chosen_release_mbid TEXT",
    "previous_mbid TEXT",
    "applied_at TEXT",
    "origin TEXT",
];

/// Create `id_reviews` and bring it up to date, in one place.
///
/// The table and the columns added to it later are two halves of one schema,
/// and anything that builds one without the other gets a table the queries
/// cannot read. That is not hypothetical: the test fixture did exactly that
/// and every review test failed on `no such column`.
#[cfg(feature = "sampo-support")]
pub(crate) fn ensure_review_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(REVIEW_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
    for column in REVIEW_COLUMNS {
        // Already-present columns fail here, which is the expected path on
        // every start after the first.
        let _ = conn.execute(&format!("ALTER TABLE id_reviews ADD COLUMN {column}"), []);
    }
    Ok(())
}

/// Recorded boundary edits, shaped like `id_reviews` for the same reason
/// `[SPEC021 §2]`: an edit changes what a passage *is*, and the library is
/// Sampo's to write, not a web click's. `tools/apply_boundary_reviews.py`
/// folds an accepted row into `passages`, on its own schedule.
///
/// No `previous_*` columns for *revert* -- unlike a recording reassignment,
/// the automatic values a manual edit overrides are always recoverable by
/// re-running the pass that produced them, so nothing here is the only copy
/// of a fact. `orig_*` below exists for a different reason `[SPEC-DF-102]`.
#[cfg(feature = "sampo-support")]
pub(crate) const BOUNDARY_REVIEW_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS boundary_reviews (
        passage_id  INTEGER PRIMARY KEY,
        start_ms    INTEGER NOT NULL,
        end_ms      INTEGER NOT NULL,
        lead_in_ms  INTEGER,
        lead_out_ms INTEGER,
        gain_db     REAL,
        decided_at  TEXT NOT NULL,
        applied_at  TEXT);";

/// The pre-edit span and the passage's file identity `[SPEC-DF-102]`,
/// captured at decision time from the passage's *current* row -- not for
/// revert, which still re-derives, but because this edit changes the
/// passage's only portable identity `[SPEC-DF-035]`. Without the span as it
/// stood before this edit, a receiving installation that has not seen the
/// edit yet has nothing stable to resolve the decision against.
#[cfg(feature = "sampo-support")]
pub(crate) const BOUNDARY_REVIEW_COLUMNS: [&str; 8] = [
    "audio_md5 TEXT",
    "orig_kind TEXT",
    "orig_start_ms INTEGER",
    "orig_end_ms INTEGER",
    "orig_lead_in_ms INTEGER",
    "orig_lead_out_ms INTEGER",
    "orig_gain_db REAL",
    "origin TEXT",
];

#[cfg(feature = "sampo-support")]
pub(crate) fn ensure_boundary_review_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(BOUNDARY_REVIEW_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
    for column in BOUNDARY_REVIEW_COLUMNS {
        let _ = conn.execute(&format!("ALTER TABLE boundary_reviews ADD COLUMN {column}"), []);
    }
    Ok(())
}

/// A correction to a recording's credited artist `[SPEC-SUI-197]` -- a
/// different table with a different key from `id_reviews`, because it
/// corrects a different table with a different key: `recording_artists` is
/// keyed `(mbid, artist_mbid)`, not `passage_id`.
///
/// **Keyed by `recording_mbid`, not `passage_id`** -- corrected from this
/// table's first version, which used `passage_id` because the correction is
/// *reached* from a passage's card. The credit belongs to the recording, and
/// the same recording can sit under several passages (any file it appears in
/// more than once); keying by `passage_id` let two different cards for the
/// same recording each record their own, silently conflicting correction.
/// It also made a synced correction `[SPEC-DF-103]` impossible to key at all,
/// since a receiver has no originating passage for a decision it never made.
/// `passage_id` is kept as a plain, non-unique column -- which card the
/// correction happened to be made from, useful for provenance, load-bearing
/// for nothing.
///
/// `artist_name` is stored, not looked up at apply time, because a name
/// found through live search (`[SPEC-SUI-196]`) exists nowhere in the
/// database to look back up later -- unlike a fingerprint suggestion, which
/// `identification_cache` already holds a name for.
/// `previous_artist_*` is what makes `apply_boundary_reviews.py`'s sibling
/// able to revert an applied correction -- captured at decision time, the
/// same reason `id_reviews.previous_mbid` is: applying overwrites the only
/// other copy. The single heaviest existing credit, same as `previous_mbid`
/// captures the single heaviest existing recording link; `NULL` when the
/// recording had no credited artist at all, which is itself worth restoring
/// to on revert rather than inventing one.
#[cfg(feature = "sampo-support")]
pub(crate) const ARTIST_REVIEW_TABLE: &str = "
    CREATE TABLE IF NOT EXISTS artist_reviews (
        recording_mbid         TEXT PRIMARY KEY,
        passage_id             INTEGER,
        artist_mbid            TEXT NOT NULL,
        artist_name            TEXT NOT NULL,
        previous_artist_mbid   TEXT,
        previous_artist_name   TEXT,
        previous_artist_weight REAL,
        decided_at             TEXT NOT NULL,
        applied_at             TEXT);";

/// `origin` `[SPEC-DF-104]`, added the same way the other two review tables
/// gain it: `NULL` for a decision made on this machine, else the hostname it
/// arrived from.
#[cfg(feature = "sampo-support")]
pub(crate) const ARTIST_REVIEW_COLUMNS: [&str; 1] = ["origin TEXT"];

#[cfg(feature = "sampo-support")]
pub(crate) fn ensure_artist_review_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(ARTIST_REVIEW_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
    for column in ARTIST_REVIEW_COLUMNS {
        let _ = conn.execute(&format!("ALTER TABLE artist_reviews ADD COLUMN {column}"), []);
    }
    Ok(())
}

pub(crate) const COLS: &str = "p.passage_id, f.path, p.start_ms, p.end_ms, f.duration_ms, \
                               p.lead_in_ms, p.lead_out_ms, p.gain_db, \
                               (SELECT pr.mbid FROM passage_recordings pr \
                                WHERE pr.passage_id = p.passage_id \
                                ORDER BY pr.weight DESC, pr.mbid LIMIT 1)";
pub(crate) const FROM: &str = "FROM passages p JOIN files f USING (file_id)";

pub(crate) fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueEntry> {
    Ok(QueueEntry {
        qid: 0, // stamped by Queue on the way in
        passage_id: row.get(0)?,
        path: PathBuf::from(row.get::<_, String>(1)?),
        start_ms: row.get::<_, i64>(2)? as u64,
        end_ms: row.get::<_, i64>(3)? as u64,
        // The file's own length `[REQ-VIS-235]`. NULL where a file was never
        // probed, which is a gap in knowledge rather than a zero-length file;
        // the interface shows it as unknown rather than as 0:00.
        file_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
        // NULL lead means "not analysed": treat as no fade rather than
        // inventing one. overlap_ms then yields zero and the handover is
        // gapless, which is the safe default [XFD-OV-010].
        lead_in_ms: row.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64,
        lead_out_ms: row.get::<_, Option<i64>>(6)?.unwrap_or(0).max(0) as u64,
        gain_db: row.get::<_, Option<f64>>(7)?.unwrap_or(0.0) as f32,
        // A scalar subquery rather than a join: a passage may legally hold a
        // medley of several recordings `[SPEC-SC-*]`, and a join would silently
        // return that passage twice. Highest weight wins, mbid breaks ties.
        mbid: row.get::<_, Option<String>>(8)?,
        naming: Naming::default(),
    })
}

impl Library {
    /// Open read-only.
    ///
    /// This is the handle everything on the *reading* path uses -- selection,
    /// naming, browsing -- and it cannot write, so a bug in any of them cannot
    /// corrupt the library. It is not a claim that the player never writes:
    /// `PlayerStore` keeps the resume row and creates `file_tags`, and the tag
    /// scan takes `open_writable` `[REQ-VIS-180]`. The guard is narrower than
    /// "only Sampo writes", which is what this comment used to say, and it is
    /// the narrow version that is true.
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| DbError::Open(e.to_string()))?;
        conn.busy_timeout(BUSY_WAIT).map_err(|e| DbError::Open(e.to_string()))?;
        Ok(Self { conn })
    }

    /// Open for writing, for the **scanner only**.
    ///
    /// The read-only default above is a real guard and stays: the player must
    /// not be able to corrupt the library. `tagscan` is a tool rather than the
    /// player -- the same standing as Sampo -- and it is the only caller here.
    pub fn open_writable(path: &std::path::Path) -> Result<Self, DbError> {
        let conn = Connection::open(path).map_err(|e| DbError::Open(e.to_string()))?;
        conn.busy_timeout(BUSY_WAIT).map_err(|e| DbError::Open(e.to_string()))?;
        Ok(Self { conn })
    }

    /// The words for a passage, if the library has them `[SPEC-LYR-040]`.
    ///
    /// **Looked up by recording, not by passage.** Two passages of one
    /// recording share its words, and a second rip does not get its own
    /// `[SPEC-LYR-020]`. An absent table is a library that predates the import,
    /// not a fault — the query is allowed to fail and mean "none".
    pub fn lyrics(&self, passage_id: i64) -> Option<String> {
        self.conn
            .query_row(
                "SELECT l.text FROM passage_recordings pr                    JOIN lyrics l ON l.mbid = pr.mbid                  WHERE pr.passage_id = ?1                  ORDER BY pr.weight DESC LIMIT 1",
                [passage_id],
                |r| r.get::<_, String>(0),
            )
            .ok()
    }

    pub fn passage(&self, passage_id: i64) -> Result<QueueEntry, DbError> {
        self.conn
            .query_row(&format!("SELECT {COLS} {FROM} WHERE p.passage_id = ?1"), [passage_id], row_to_entry)
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Radio passages in random order — a stand-in until the Program Director
    /// is wired in `[SPEC009]`. Radio only, per `[REQ-PD-120]`.
    pub fn random_radio(&self, limit: usize) -> Result<Vec<QueueEntry>, DbError> {
        let sql = format!("SELECT {COLS} {FROM} WHERE p.kind = 'radio' ORDER BY RANDOM() LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = stmt
            .query_map([limit as i64], row_to_entry)
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| DbError::Query(e.to_string()))
    }

    /// Load the Program Director from this same library `[SPEC009]`.
    /// Keeps the connection private -- selection reads the library, it does
    /// not get its own handle on the file.
    /// Names and play count for one passage, from MusicBrainz.
    ///
    /// Silent failure on purpose: a passage whose names cannot be read still
    /// plays, and still shows its filename. Nothing here is worth interrupting
    /// the music for.
    /// The file's own tags, if they have been scanned.
    pub fn stored_tags(&self, passage_id: i64) -> Option<crate::tags::Tags> {
        self.conn
            .query_row(
                "SELECT t.title, t.artist, t.album FROM passages p \
                   JOIN file_tags t ON t.file_id = p.file_id \
                  WHERE p.passage_id = ?1",
                [passage_id],
                |r| {
                    Ok(crate::tags::Tags {
                        title: r.get(0)?,
                        artist: r.get(1)?,
                        album: r.get(2)?,
                        track_no: None,
                        disc_no: None,
                    })
                },
            )
            .ok()
    }

    pub fn describe(&self, e: &mut QueueEntry) {
        let Some(mbid) = e.mbid.clone() else { return };
        let got = self.conn.query_row(DESCRIBE, [&mbid], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        });
        if let Ok((title, artist, album, plays, last)) = got {
            e.naming.mb_title = title;
            e.naming.mb_artist = artist;
            e.naming.mb_album = album;
            e.naming.plays = plays;
            e.naming.last_played = last;
        }
    }

    /// Where a passage's audio lives, for serving its cover art.
    pub fn passage_path(&self, passage_id: i64) -> Result<std::path::PathBuf, DbError> {
        let p: String = self.conn.query_row(
            "SELECT f.path FROM passages p JOIN files f ON f.file_id = p.file_id              WHERE p.passage_id = ?1",
            [passage_id],
            |r| r.get(0),
        )
        .map_err(|e| DbError::Query(e.to_string()))?;
        Ok(std::path::PathBuf::from(p))
    }

    /// Remember what a file's own tags say `[REQ-VIS-180]`.
    ///
    /// Reading tags means opening and probing the file, which is far too slow
    /// to do for a whole library on demand -- and browsing by album has no
    /// other source at all, the release tables being empty. So the answers are
    /// kept. The table is the player's own, created here rather than by the
    /// ingest tools, because it is the player that needs it.
    pub fn ensure_tag_table(&self) -> Result<(), DbError> {
        self.conn.execute_batch(TAG_TABLE).map_err(|e| DbError::Query(e.to_string()))?;
        // An index built before track numbers existed has the rows but not the
        // columns. Adding a column succeeds exactly once; on that run the
        // stored tags are dropped so the background scan reads the numbers in
        // `[REQ-VIS-190]`. Cheaper than a version table for one migration, and
        // it cannot half-apply.
        // Sampo marks the release it chose for a recording `[SPEC-SA-030]`.
        // Created here so a library Sampo has never touched still browses:
        // the album expression orders by this column, and a missing one is a
        // failed query rather than an empty result.
        let _ = self
            .conn
            .execute("ALTER TABLE release_recordings ADD COLUMN chosen INTEGER DEFAULT 0", []);
        for column in ["track_no", "disc_no"] {
            let added = self
                .conn
                .execute(&format!("ALTER TABLE file_tags ADD COLUMN {column} INTEGER"), [])
                .is_ok();
            if added {
                let _ = self.conn.execute("DELETE FROM file_tags", []);
            }
        }
        Ok(())
    }

    pub fn put_tags(
        &self,
        file_id: i64,
        t: &crate::tags::Tags,
        has_art: bool,
    ) -> Result<(), DbError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.conn
            .execute(
                "INSERT INTO file_tags \
                     (file_id, title, artist, album, track_no, disc_no, has_art, scanned_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT(file_id) DO UPDATE SET \
                   title = ?2, artist = ?3, album = ?4, track_no = ?5, disc_no = ?6, \
                   has_art = ?7, scanned_at = ?8",
                rusqlite::params![
                    file_id,
                    t.title,
                    t.artist,
                    t.album,
                    t.track_no,
                    t.disc_no,
                    has_art as i64,
                    now
                ],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Throw away the tag index, so a rescan reads every file again.
    pub fn forget_tags(&self) -> Result<(), DbError> {
        self.ensure_tag_table()?;
        self.conn
            .execute("DELETE FROM file_tags", [])
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Files with no tag row yet. What a resumed or incremental scan works on:
    /// re-reading five thousand files to learn nothing new is the difference
    /// between a scan that can run at startup and one that cannot.
    pub fn files_without_tags(&self) -> Result<Vec<(i64, std::path::PathBuf)>, DbError> {
        let mut st = self
            .conn
            .prepare(
                "SELECT f.file_id, f.path FROM files f \
                   LEFT JOIN file_tags t ON t.file_id = f.file_id \
                  WHERE t.file_id IS NULL ORDER BY f.file_id",
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, std::path::PathBuf::from(r.get::<_, String>(1)?)))
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Every file, for the scanner. Path included so it can be read.
    pub fn all_files(&self) -> Result<Vec<(i64, std::path::PathBuf)>, DbError> {
        let mut st = self
            .conn
            .prepare("SELECT file_id, path FROM files ORDER BY file_id")
            .map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, std::path::PathBuf::from(r.get::<_, String>(1)?)))
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    pub fn director(&self) -> Result<crate::director::library::Director, DbError> {
        crate::director::library::Director::load(&self.conn)
    }

    pub fn count_radio(&self) -> Result<i64, DbError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM passages WHERE kind = 'radio'", [], |r| r.get(0))
            .map_err(|e| DbError::Query(e.to_string()))
    }
}

/// Read-write access to the one row of state the player owns.
///
/// Separate from [`Library`] on purpose: the library is opened read-only so a
/// player bug cannot corrupt it, and the writable surface stays visibly tiny.
/// Play history will be written here too, but only once `QueueEntry` carries
/// the recording MBID it must be keyed by `[SPEC-SC-095]` — an untested writer
/// with no reader would be a claim nothing exercises.
pub struct PlayerStore {
    conn: Connection,
}

/// Every setting the listener owns `[REQ-VIS-155]`, in one row.
///
/// A struct rather than a tuple because there are eight of them now and a
/// nine-element tuple is a way to swap two `u64`s without the compiler minding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub volume: f32,
    pub skip_fade_ms: u64,
    pub skip_lead_ms: u64,
    pub resume_save_ms: u64,
    /// `[SPEC-PLAY-050]`
    pub skip_suppress_h: u64,
    /// `[SPEC-PLAY-055]`
    pub dequeue_suppress_h: u64,
    /// How many passages to keep ahead. Governs the local engine and the MPD
    /// Director alike `[SPEC-MPD-105]`.
    pub queue_depth: usize,
    /// How often `status` is read while playing `[SPEC-MPD-105]`.
    pub sample_interval_ms: u64,
    /// Whether Vaino may write cue sheets into the music folder so a guest can
    /// name a passage inside a capture `[REQ-VIS-205]`. **Off by default**:
    /// nothing else in Vaino puts a file there.
    pub cue_sheets: bool,
    /// Whether Vaino may write cover art into the music folder so a guest can
    /// show the album `[REQ-VIS-210]`. **Off by default**, for the same reason.
    pub covers: bool,
    /// Whether Vaino may write per-song lyrics into a local client's cache
    /// `[REQ-VIS-215]`. **Off by default**, and useful only where that client
    /// runs on this machine `[SPEC-LYR-075]`.
    pub lyrics_cache: bool,
    /// Whether Vaino may write lyrics beside the audio `[REQ-VIS-220]`.
    /// **Off by default**: it writes into the music folder, and it is
    /// deliberately blind to captures `[SPEC-LYR-080]`.
    pub lyrics_sidecar: bool,
}

impl Settings {
    /// Every persisted setting, by the name it is stored under
    /// `[SPEC-SC-099]`.
    ///
    /// **The names are the old column names on purpose**, so a database written
    /// before this carries over without a translation table, and so anyone
    /// reading `player_settings` sees what they expect.
    ///
    /// This list is the single source: `value_of` and `set` are both matched
    /// against it by `every_setting_survives_a_round_trip`, so a field added to
    /// one and forgotten in the other fails a test rather than silently losing
    /// itself on the next restart.
    pub const KEYS: [&'static str; 12] = [
        "volume",
        "skip_fade_ms",
        "skip_lead_ms",
        "resume_save_ms",
        "skip_suppress_h",
        "dequeue_suppress_h",
        "queue_depth",
        "sample_interval_ms",
        "cue_sheets",
        "covers",
        "lyrics_cache",
        "lyrics_sidecar",
    ];

    /// What to store under `key`, or `None` if this is not a setting.
    fn value_of(&self, key: &str) -> Option<String> {
        Some(match key {
            "volume" => self.volume.to_string(),
            "skip_fade_ms" => self.skip_fade_ms.to_string(),
            "skip_lead_ms" => self.skip_lead_ms.to_string(),
            "resume_save_ms" => self.resume_save_ms.to_string(),
            "skip_suppress_h" => self.skip_suppress_h.to_string(),
            "dequeue_suppress_h" => self.dequeue_suppress_h.to_string(),
            "queue_depth" => self.queue_depth.to_string(),
            "sample_interval_ms" => self.sample_interval_ms.to_string(),
            "cue_sheets" => (self.cue_sheets as i64).to_string(),
            "covers" => (self.covers as i64).to_string(),
            "lyrics_cache" => (self.lyrics_cache as i64).to_string(),
            "lyrics_sidecar" => (self.lyrics_sidecar as i64).to_string(),
            _ => return None,
        })
    }

    /// Take one stored value. Anything unreadable leaves the field alone, so a
    /// corrupted row costs that setting and not the rest of them.
    fn set(&mut self, key: &str, value: &str) {
        // Booleans were written as 0/1 by the old columns and still are.
        let flag = || matches!(value, "1" | "true");
        match key {
            "volume" => {
                if let Ok(v) = value.parse() {
                    self.volume = v;
                }
            }
            "skip_fade_ms" => set_u64(&mut self.skip_fade_ms, value),
            "skip_lead_ms" => set_u64(&mut self.skip_lead_ms, value),
            "resume_save_ms" => set_u64(&mut self.resume_save_ms, value),
            "skip_suppress_h" => set_u64(&mut self.skip_suppress_h, value),
            "dequeue_suppress_h" => set_u64(&mut self.dequeue_suppress_h, value),
            "queue_depth" => {
                if let Ok(v) = value.parse() {
                    self.queue_depth = v;
                }
            }
            "sample_interval_ms" => set_u64(&mut self.sample_interval_ms, value),
            "cue_sheets" => self.cue_sheets = flag(),
            "covers" => self.covers = flag(),
            "lyrics_cache" => self.lyrics_cache = flag(),
            "lyrics_sidecar" => self.lyrics_sidecar = flag(),
            _ => {}
        }
    }
}

fn set_u64(field: &mut u64, value: &str) {
    if let Ok(v) = value.parse() {
        *field = v;
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            skip_fade_ms: crate::SKIP_FADE_MS,
            skip_lead_ms: crate::SKIP_LEAD_MS,
            resume_save_ms: crate::RESUME_SAVE_MS,
            skip_suppress_h: crate::SKIP_SUPPRESS_H,
            dequeue_suppress_h: crate::DEQUEUE_SUPPRESS_H,
            queue_depth: crate::QUEUE_DEPTH,
            sample_interval_ms: crate::SAMPLE_INTERVAL_MS,
            cue_sheets: false,
            covers: false,
            lyrics_cache: false,
            lyrics_sidecar: false,
        }
    }
}

/// The two ways a listener declines a passage `[SPEC-PLAY-055]`.
///
/// They differ only in the window they earn. A **skip** is a passage stopped
/// after it began sounding; a **dequeue** is one removed from the queue before
/// it ever played, which is the weaker statement and gets the shorter window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Skip,
    Dequeue,
}

impl Rejection {
    pub fn as_str(self) -> &'static str {
        match self {
            Rejection::Skip => "skip",
            Rejection::Dequeue => "dequeue",
        }
    }
}

impl PlayerStore {
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let conn = Connection::open(path).map_err(|e| DbError::Open(e.to_string()))?;
        conn.busy_timeout(BUSY_WAIT).map_err(|e| DbError::Open(e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS player_state (
                 id INTEGER PRIMARY KEY CHECK (id = 1),
                 passage_id INTEGER, position_ms INTEGER NOT NULL DEFAULT 0,
                 playing INTEGER NOT NULL DEFAULT 0, volume REAL NOT NULL DEFAULT 1.0,
                 updated_at TEXT NOT NULL);",
        )
        .map_err(|e| DbError::Open(e.to_string()))?;
        // Browsing joins this table, and an absent one fails the query rather
        // than returning nothing. Created here because this is the player's
        // only writable handle; filling it is `tagscan`'s job.
        conn.execute_batch(TAG_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
        // Same reasoning as the tag table above: created by whoever holds a
        // writable handle, so the read path never meets a missing table.
        // Filling it is `tools/fetch_cover_art.py`'s job.
        conn.execute_batch(ART_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
        conn.execute_batch(PLAY_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
        conn.execute_batch(REJECTION_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
        conn.execute_batch(FLAGS_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
        ensure_history_columns(&conn);
        ensure_flags_columns(&conn);
        #[cfg(feature = "sampo-support")]
        ensure_review_table(&conn)?;
        #[cfg(feature = "sampo-support")]
        ensure_boundary_review_table(&conn)?;
        #[cfg(feature = "sampo-support")]
        ensure_artist_review_table(&conn)?;
        // Columns Sampo fills and the browse queries read `[SPEC-SA-030]`.
        // Created HERE, on every start, rather than in `ensure_tag_table`:
        // that only runs behind the background scan, so a library whose scan
        // was already complete never reached it and browsing died on a missing
        // column. A query naming a column that does not exist fails outright;
        // it does not return nothing.
        // Settings live in `player_settings`, one row each `[SPEC-SC-099]`.
        // They used to be columns on `player_state`, added by ALTER as each was
        // invented, and read back by position -- `?11` here, `r.get(10)` there.
        // Adding one meant renumbering both, and getting it wrong loaded the
        // wrong value silently rather than failing.
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS player_settings (
                 key        TEXT PRIMARY KEY,
                 value      TEXT NOT NULL,
                 updated_at TEXT NOT NULL)",
            [],
        );
        Self::adopt_old_settings_columns(&conn);
        for column in ["chosen INTEGER DEFAULT 0", "position INTEGER", "disc INTEGER"] {
            let _ = conn.execute(
                &format!("ALTER TABLE release_recordings ADD COLUMN {column}"), []);
        }
        // Album names are looked up BY RECORDING, and `release_recordings` is
        // keyed `(release_mbid, mbid)` -- so the lookup uses the second column
        // of the primary key and no index applies. SQLite falls back to a full
        // scan of the table, once per passage.
        //
        // That was free when the table was empty and became quadratic the
        // moment Sampo filled it: at 304,334 rows against 8,078 passages,
        // browsing albums went past 400 seconds and the review queue took 229.
        // With this index the review query is 0.50 s. Nothing about the code
        // changed in between, only the amount of data, which is the kind of
        // regression that arrives without a commit to blame.
        //
        // Created here because this is the player's only writable handle, and
        // on every start rather than behind a scan, for the same reason the
        // columns above are `[REQ-VIS-180]`.
        // **And the sort after it, which the single-column index did not**
        // `[PI-CHR-065]`. Measured on the appliance against the real library:
        // `browse_albums` picks the chosen release per recording, so it runs
        // `ORDER BY rr.chosen DESC, rel.release_date, rel.title LIMIT 1` once
        // per passage over some thirty-six releases each. Finding the rows was
        // already cheap; ordering them was 18.05 s of a 25.7 s page.
        //
        // Carrying `chosen` and `release_mbid` in the index lets SQLite take the
        // first row instead of sorting them: **0.93 s for the same 694 albums**,
        // built once in 4.4 s. The sort cannot simply be dropped — without it
        // the answer is 1,698 albums, because then any release will do.
        //
        // Created after `chosen` exists, which the ALTER above guarantees. That
        // is why this lives here and not in `schema.sql`: that file describes
        // the table as it is before the column this sorts on is added to it.
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_release_recordings_chosen \
               ON release_recordings(mbid, chosen DESC, release_mbid)", []);
        // Redundant once the covering one exists — `mbid` leads both, so
        // anything the old index served the new one serves too. Dropped after
        // it and never before, so there is no moment with neither.
        let _ = conn.execute("DROP INDEX IF EXISTS idx_release_recordings_mbid", []);
        Ok(Self { conn })
    }

    /// Record a judgement about a questionable id `[REQ-LIB-165]`.
    ///
    /// The decision is validated here rather than trusted from the request:
    /// this is the only writer, so it is the only place the vocabulary can be
    /// enforced. A reassignment must name a recording; a keep must not.
    #[cfg(feature = "sampo-support")]
    pub fn record_review(
        &self,
        passage_id: i64,
        decision: &str,
        chosen_mbid: Option<&str>,
        chosen_release_mbid: Option<&str>,
    ) -> Result<(), DbError> {
        let mbid = match decision {
            "reassigned" => match chosen_mbid.filter(|m| !m.is_empty()) {
                Some(m) => Some(m),
                None => {
                    return Err(DbError::Query(
                        "a reassignment has to say which recording".into(),
                    ))
                }
            },
            "kept" | "deferred" => None,
            other => return Err(DbError::Query(format!("unknown decision {other:?}"))),
        };
        let release = chosen_release_mbid.filter(|m| !m.is_empty() && decision == "reassigned");

        // Changing a judgement that has already been written into the library
        // is not a matter of overwriting a row -- the old id has to be put back
        // first, and only `apply_reviews` may touch `passage_recordings`.
        let applied: Option<String> = self
            .conn
            .query_row("SELECT applied_at FROM id_reviews WHERE passage_id = ?1",
                       [passage_id], |r| r.get(0))
            .unwrap_or(None);
        if applied.is_some() {
            return Err(DbError::Query(
                "this decision has already been applied to the library; \
                 revert it with tools/apply_reviews.py --revert before changing it"
                    .into(),
            ));
        }

        // What the passage says NOW, captured before anything replaces it. An
        // applied reassignment overwrites the only copy of the old id, so
        // without this an undo would have nothing to restore.
        let previous: Option<String> = self
            .conn
            .query_row(
                "SELECT mbid FROM passage_recordings WHERE passage_id = ?1 \
                  ORDER BY weight DESC, mbid LIMIT 1",
                [passage_id], |r| r.get(0))
            .ok();

        self.conn
            .execute(
                "INSERT INTO id_reviews
                     (passage_id, decision, chosen_mbid, chosen_release_mbid,
                      previous_mbid, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
                 ON CONFLICT(passage_id) DO UPDATE SET
                     decision = excluded.decision,
                     chosen_mbid = excluded.chosen_mbid,
                     chosen_release_mbid = excluded.chosen_release_mbid,
                     previous_mbid = excluded.previous_mbid,
                     decided_at = excluded.decided_at",
                rusqlite::params![passage_id, decision, mbid, release, previous],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Withdraw a judgement `[REQ-LIB-165]`.
    ///
    /// Only one that has not been applied. Once `apply_reviews` has rewritten
    /// `passage_recordings`, deleting the review row would strand the library
    /// on a change with no record of why it was made or what it replaced --
    /// an undo that leaves the thing it was undoing in place. Reverting an
    /// applied decision is `apply_reviews --revert`, which puts the old id
    /// back and clears the row in one transaction.
    #[cfg(feature = "sampo-support")]
    pub fn clear_review(&self, passage_id: i64) -> Result<(), DbError> {
        let applied: Option<String> = self
            .conn
            .query_row("SELECT applied_at FROM id_reviews WHERE passage_id = ?1",
                       [passage_id], |r| r.get(0))
            .map_err(|_| DbError::Query("no decision recorded for that passage".into()))?;
        if applied.is_some() {
            return Err(DbError::Query(
                "already applied to the library; use tools/apply_reviews.py --revert".into(),
            ));
        }
        self.conn
            .execute("DELETE FROM id_reviews WHERE passage_id = ?1", [passage_id])
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Record a boundary edit `[REQ-LIB-175]`, `[SPEC021 §2]`.
    ///
    /// Committing twice on the same passage updates the one row rather than
    /// adding a second -- the same `ON CONFLICT` shape `record_review` uses,
    /// for the same reason: the queue key is the passage, and a second draft
    /// is not a second finding.
    #[cfg(feature = "sampo-support")]
    pub fn record_boundary_review(
        &self,
        passage_id: i64,
        start_ms: u64,
        end_ms: u64,
        lead_in_ms: u64,
        lead_out_ms: u64,
        gain_db: f64,
    ) -> Result<(), DbError> {
        if start_ms >= end_ms {
            return Err(DbError::Query("start must come before end".into()));
        }

        // Changing a decision already folded into `passages` is not an
        // overwrite of one row -- `apply_boundary_reviews.py` has to put the
        // old span back first, the same guard `record_review` and
        // `record_artist_review` already apply to their own tables. Missing
        // here until this same change added `orig_*`: re-committing after
        // apply would have captured the *already-applied* values as if they
        // were the original, corrupting the one thing `[SPEC-DF-102]` exists
        // to keep honest.
        let applied: Option<String> = self
            .conn
            .query_row(
                "SELECT applied_at FROM boundary_reviews WHERE passage_id = ?1",
                [passage_id],
                |r| r.get(0),
            )
            .unwrap_or(None);
        if applied.is_some() {
            return Err(DbError::Query(
                "this edit has already been applied to the library".into(),
            ));
        }

        // The pre-edit span and file identity, read from `passages` itself
        // rather than from any earlier `boundary_reviews` row `[SPEC-DF-102]`.
        // `passages` does not change until `apply_boundary_reviews.py` runs,
        // so a second commit before that reads the same true original --
        // exactly how `record_review`'s `previous_mbid` stays pinned across
        // repeated commits without needing to remember its own prior value.
        let (audio_md5, orig_kind, orig_start_ms, orig_end_ms, orig_lead_in_ms, orig_lead_out_ms, orig_gain_db): (String, String, i64, i64, Option<i64>, Option<i64>, Option<f64>) = self
            .conn
            .query_row(
                "SELECT f.audio_md5, p.kind, p.start_ms, p.end_ms, p.lead_in_ms, p.lead_out_ms, p.gain_db \
                   FROM passages p JOIN files f ON f.file_id = p.file_id \
                  WHERE p.passage_id = ?1",
                [passage_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .map_err(|_| DbError::Query("no such passage".into()))?;

        self.conn
            .execute(
                "INSERT INTO boundary_reviews
                     (passage_id, start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db,
                      audio_md5, orig_kind, orig_start_ms, orig_end_ms,
                      orig_lead_in_ms, orig_lead_out_ms, orig_gain_db, decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, datetime('now'))
                 ON CONFLICT(passage_id) DO UPDATE SET
                     start_ms = excluded.start_ms,
                     end_ms = excluded.end_ms,
                     lead_in_ms = excluded.lead_in_ms,
                     lead_out_ms = excluded.lead_out_ms,
                     gain_db = excluded.gain_db,
                     audio_md5 = excluded.audio_md5,
                     orig_kind = excluded.orig_kind,
                     orig_start_ms = excluded.orig_start_ms,
                     orig_end_ms = excluded.orig_end_ms,
                     orig_lead_in_ms = excluded.orig_lead_in_ms,
                     orig_lead_out_ms = excluded.orig_lead_out_ms,
                     orig_gain_db = excluded.orig_gain_db,
                     decided_at = excluded.decided_at",
                rusqlite::params![
                    passage_id, start_ms as i64, end_ms as i64,
                    lead_in_ms as i64, lead_out_ms as i64, gain_db,
                    audio_md5, orig_kind, orig_start_ms, orig_end_ms,
                    orig_lead_in_ms, orig_lead_out_ms, orig_gain_db,
                ],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Correct a recording's credited artist without touching the recording
    /// itself `[SPEC-SUI-197]` -- the one correction today's review page
    /// cannot make any other way.
    ///
    /// Refuses a passage with no recording linked at all: "the recording is
    /// right, the credit is wrong" presupposes a recording has been chosen,
    /// and there is nothing here to attach a corrected credit to otherwise.
    #[cfg(feature = "sampo-support")]
    pub fn record_artist_review(
        &self,
        passage_id: i64,
        artist_mbid: &str,
        artist_name: &str,
    ) -> Result<(), DbError> {
        let recording_mbid: String = self
            .conn
            .query_row(
                "SELECT mbid FROM passage_recordings WHERE passage_id = ?1 \
                  ORDER BY weight DESC, mbid LIMIT 1",
                [passage_id],
                |r| r.get(0),
            )
            .map_err(|_| {
                DbError::Query("this passage has no recording linked yet".into())
            })?;

        // Keyed by `recording_mbid`, not `passage_id`: the credit belongs to
        // the recording, and a second passage carrying the same recording
        // must find and refuse (or update) the SAME row, not start a second,
        // silently conflicting one.
        let applied: Option<String> = self
            .conn
            .query_row(
                "SELECT applied_at FROM artist_reviews WHERE recording_mbid = ?1",
                [&recording_mbid],
                |r| r.get(0),
            )
            .unwrap_or(None);
        if applied.is_some() {
            return Err(DbError::Query(
                "this correction has already been applied to the library".into(),
            ));
        }

        // Captured now, because applying overwrites the only other copy --
        // `None` when the recording carries no credit at all today, which is
        // itself the state a revert should restore rather than inventing one.
        let previous: Option<(String, String, f64)> = self
            .conn
            .query_row(
                "SELECT ra.artist_mbid, a.name, ra.weight \
                   FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid \
                  WHERE ra.mbid = ?1 ORDER BY ra.weight DESC, a.name LIMIT 1",
                [&recording_mbid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let (prev_mbid, prev_name, prev_weight) = match previous {
            Some((m, n, w)) => (Some(m), Some(n), Some(w)),
            None => (None, None, None),
        };

        self.conn
            .execute(
                "INSERT INTO artist_reviews
                     (recording_mbid, passage_id, artist_mbid, artist_name,
                      previous_artist_mbid, previous_artist_name, previous_artist_weight,
                      decided_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))
                 ON CONFLICT(recording_mbid) DO UPDATE SET
                     passage_id = excluded.passage_id,
                     artist_mbid = excluded.artist_mbid,
                     artist_name = excluded.artist_name,
                     previous_artist_mbid = excluded.previous_artist_mbid,
                     previous_artist_name = excluded.previous_artist_name,
                     previous_artist_weight = excluded.previous_artist_weight,
                     decided_at = excluded.decided_at",
                rusqlite::params![
                    recording_mbid, passage_id, artist_mbid, artist_name,
                    prev_mbid, prev_name, prev_weight,
                ],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Withdraw an artist correction, the same "recorded, not yet applied"
    /// undo `clear_review` offers for a recording reassignment.
    #[cfg(feature = "sampo-support")]
    pub fn clear_artist_review(&self, passage_id: i64) -> Result<(), DbError> {
        // Resolved through the passage's CURRENT recording link, the same
        // way `record_artist_review` finds the row to begin with -- the
        // undo button lives on a passage's card, but the row it withdraws is
        // keyed by the recording that card currently names.
        let recording_mbid: String = self
            .conn
            .query_row(
                "SELECT mbid FROM passage_recordings WHERE passage_id = ?1 \
                  ORDER BY weight DESC, mbid LIMIT 1",
                [passage_id],
                |r| r.get(0),
            )
            .map_err(|_| DbError::Query("this passage has no recording linked".into()))?;
        let applied: Option<String> = self
            .conn
            .query_row(
                "SELECT applied_at FROM artist_reviews WHERE recording_mbid = ?1",
                [&recording_mbid],
                |r| r.get(0),
            )
            .map_err(|_| DbError::Query("no artist correction recorded for that recording".into()))?;
        if applied.is_some() {
            return Err(DbError::Query(
                "already applied to the library; use tools/apply_reviews.py --revert-artist".into(),
            ));
        }
        self.conn
            .execute("DELETE FROM artist_reviews WHERE recording_mbid = ?1", [&recording_mbid])
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Save the resume point `[REQ-AUD-140]`.
    /// The listener's settings `[REQ-VIS-155]`.
    ///
    /// Separate from `save`, which runs every second to keep the resume point
    /// current. These change when someone moves a control and not otherwise,
    /// so writing them on that schedule would be a write per second to record
    /// that nothing had happened.
    ///
    /// Volume was already a column here and was never written to it -- the
    /// resume point saved position and playing state and quietly left the
    /// level behind, so it came back at full scale every start.
    pub fn save_settings(&self, s: &Settings) -> Result<(), DbError> {
        for key in Settings::KEYS {
            let Some(value) = s.value_of(key) else { continue };
            self.conn
                .execute(
                    "INSERT INTO player_settings (key, value, updated_at)
                     VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(key) DO UPDATE SET
                         value = excluded.value, updated_at = excluded.updated_at",
                    rusqlite::params![key, value],
                )
                .map_err(|e| DbError::Query(e.to_string()))?;
        }
        Ok(())
    }

    /// The settings as they were left.
    ///
    /// A key that is absent, or holds something unreadable, falls back to its
    /// **own** default rather than failing the whole read: an absent key is a
    /// setting never touched, which is a first run and not a fault.
    pub fn load_settings(&self) -> Option<Settings> {
        let mut out = Settings::default();
        let mut q = self.conn.prepare("SELECT key, value FROM player_settings").ok()?;
        let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).ok()?;
        let mut any = false;
        for (key, value) in rows.flatten() {
            out.set(&key, &value);
            any = true;
        }
        // **`None` means never saved**, which is what a caller distinguishes a
        // first run by. Returning the defaults instead would be the same values
        // wearing a claim that somebody chose them.
        any.then_some(out)
    }

    /// Bring `listener_settings.utc_offset_minutes` into line with what the
    /// OS believes right now, if it differs `[SPEC-DIR-180]`, `[REQ-VIS-255]`.
    ///
    /// **Must run through this connection, not `Library`'s.** `Library::open`
    /// is deliberately read-only ("the player must not be able to corrupt the
    /// library"), so a write attempted there fails silently every time --
    /// which is exactly the trap the first version of this fix fell into: it
    /// looked like it was working, called `sync_os_utc_offset` on schedule,
    /// and never once reached the disk. `PlayerStore` is the one connection
    /// this process holds that can actually write.
    pub fn sync_utc_offset(&self) {
        crate::director::program::sync_os_utc_offset(&self.conn);
    }

    /// Remember which speaker was chosen `[PI3-AIM-020]`, `[REQ-VIS-260]`.
    ///
    /// Found by its absence: the appliance's own reconnect timer had no way
    /// to know which of possibly several trusted, paired devices was the one
    /// actually in use, so it kept paging a stale one left over from early
    /// testing -- and paging a device the shared radio cannot reach stalls
    /// whatever *is* playing for several seconds, audible as a skip and
    /// invisible to every counter Vaino already had, because the stall never
    /// touches the output ring at all.
    ///
    /// Lives in `player_settings`, the same table the settings round-trip
    /// uses, but kept out of `Settings` deliberately: nothing in the panel
    /// reads this back, and folding it into that struct's whole-round-trip
    /// contract would ask `every_setting_survives_a_round_trip` to police a
    /// field the panel does not show.
    pub fn save_speaker_address(&self, address: &str) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO player_settings (key, value, updated_at)
                 VALUES ('speaker_address', ?1, datetime('now'))
                 ON CONFLICT(key) DO UPDATE SET
                     value = excluded.value, updated_at = excluded.updated_at",
                rusqlite::params![address],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// The speaker last chosen through `use` or `pair`, if any
    /// `[PI3-AIM-020]`, `[REQ-VIS-260]`. `None` on a library where nothing
    /// has ever been chosen this way -- the caller's job is to fall back to
    /// doing nothing, not to invent an address.
    pub fn load_speaker_address(&self) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM player_settings WHERE key = 'speaker_address'",
                [],
                |r| r.get(0),
            )
            .ok()
    }

    /// Set or clear "flag this for review" on a recording or a passage
    /// `[REQ-VIS-265]`. A plain toggle, not a decision: unlike `id_reviews`
    /// and its siblings, there is nothing here to apply and nothing to
    /// refuse -- checking and unchecking the box are the same operation with
    /// the state reversed, at any time, by design.
    pub fn set_flag(&self, subject_kind: &str, subject_id: &str, flagged: bool) -> Result<(), DbError> {
        if flagged {
            self.conn
                .execute(
                    "INSERT INTO listener_flags (subject_kind, subject_id, flagged_at)
                     VALUES (?1, ?2, datetime('now'))
                     ON CONFLICT(subject_kind, subject_id) DO UPDATE SET
                         flagged_at = excluded.flagged_at",
                    rusqlite::params![subject_kind, subject_id],
                )
                .map(|_| ())
        } else {
            self.conn
                .execute(
                    "DELETE FROM listener_flags WHERE subject_kind = ?1 AND subject_id = ?2",
                    rusqlite::params![subject_kind, subject_id],
                )
                .map(|_| ())
        }
        .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Bring settings over from the columns they used to live in.
    ///
    /// Runs once: after the first save every key is present and the copy is
    /// skipped. A library from before the columns existed fails the read and
    /// keeps its defaults, which is the same answer by a shorter road.
    fn adopt_old_settings_columns(conn: &Connection) {
        let already: i64 = conn
            .query_row("SELECT COUNT(*) FROM player_settings", [], |r| r.get(0))
            .unwrap_or(0);
        if already > 0 {
            return;
        }
        let mut moved = 0;
        for key in Settings::KEYS {
            // One column at a time, because a database part-way through the old
            // sequence of ALTERs has some of them and not others.
            // As `Value`, not as `String`: `volume` is REAL and the rest are
            // INTEGER, and asking SQLite for a number as text fails the read
            // rather than converting it.
            let got: Option<rusqlite::types::Value> = conn
                .query_row(
                    &format!("SELECT {key} FROM player_state WHERE id = 1"),
                    [],
                    |r| r.get::<_, rusqlite::types::Value>(0),
                )
                .ok();
            let text = match got {
                Some(rusqlite::types::Value::Integer(i)) => Some(i.to_string()),
                Some(rusqlite::types::Value::Real(f)) => Some(f.to_string()),
                Some(rusqlite::types::Value::Text(t)) => Some(t),
                _ => None,
            };
            if let Some(value) = text {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO player_settings (key, value, updated_at)
                     VALUES (?1, ?2, datetime('now'))",
                    rusqlite::params![key, value],
                );
                moved += 1;
            }
        }
        if moved > 0 {
            println!("settings: {moved} carried over from the old columns");
        }
    }
    pub fn save(&self, passage_id: Option<i64>, position_ms: u64, playing: bool)
        -> Result<(), DbError>
    {
        self.conn
            .execute(
                "INSERT INTO player_state (id, passage_id, position_ms, playing, updated_at)
                 VALUES (1, ?1, ?2, ?3, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                     passage_id = excluded.passage_id,
                     position_ms = excluded.position_ms,
                     playing = excluded.playing,
                     updated_at = excluded.updated_at",
                rusqlite::params![passage_id, position_ms as i64, playing as i64],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// The durable record of one selection `[SPEC-DIR-190]`.
    ///
    /// `detail` is the full decomposition as JSON. A failure here must never
    /// stop the music: the record is for explaining a choice afterwards, not
    /// for making it.
    pub fn record_decision(&self, at: i64, passage_id: i64, detail: &str) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO selection_decisions (selected_at, passage_id, detail)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![at, passage_id, detail],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Record that a passage played `[REQ-PD-110]`.
    ///
    /// `mbid` is stored alongside `passage_id` because six years of history
    /// must survive a rescan that renumbers passages `[SPEC-SC-095]`; the
    /// passage id is the convenience, the MBID is the durable key.
    /// Record a play, keyed by recording MBID `[REQ-PD-112]`.
    ///
    /// Rotation is meaningless without it: an unrecorded play leaves a track as
    /// eligible as it was before it was heard.
    ///
    /// `heard_ms` is the threshold just crossed, not the final figure -- the
    /// caller corrects it with [`finish_play`](Self::finish_play) once the
    /// passage actually departs `[REQ-VIS-250]`. Returns the row's id so the
    /// caller can do that.
    pub fn record_play(
        &self,
        passage_id: i64,
        mbid: Option<&str>,
        heard_ms: u64,
        span_ms: u64,
    ) -> Result<i64, DbError> {
        self.conn
            .execute(
                "INSERT INTO listener_play_history (played_at, passage_id, mbid, heard_ms, span_ms) \
                 VALUES (strftime('%s','now'), ?1, ?2, ?3, ?4)",
                rusqlite::params![passage_id, mbid, heard_ms, span_ms],
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Fill in how much of a play was actually heard, now that the passage has
    /// departed and the figure will not grow again `[REQ-VIS-250]`.
    pub fn finish_play(&self, play_id: i64, heard_ms: u64) -> Result<(), DbError> {
        self.conn
            .execute(
                "UPDATE listener_play_history SET heard_ms = ?1 WHERE play_id = ?2",
                rusqlite::params![heard_ms, play_id],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Record a rejection, for suppression and nothing else `[SPEC-PLAY-050]`.
    ///
    /// A passage the listener declined did not play, so it must not enter
    /// `listener_play_history` — but offering it back an hour later is its own
    /// kind of wrong. This is the narrowest record that fixes that: a timestamp
    /// per recording per kind, read only by the eligibility gate, feeding no
    /// ramp, no artist damping and no count.
    ///
    /// **The window is applied when the gate runs, not stored here**
    /// `[SPEC-PLAY-055]`. Keeping the instant and the kind rather than an expiry
    /// is what lets the listener change a window and have it apply to what they
    /// have already rejected; an expiry computed under yesterday's setting would
    /// outlive the setting itself.
    /// `heard_ms`/`span_ms` are `None` for a dequeue -- the passage never
    /// sounded, so there is nothing to report a percentage of
    /// `[REQ-VIS-250]`.
    pub fn record_rejection(
        &self,
        kind: Rejection,
        passage_id: i64,
        mbid: Option<&str>,
        heard_ms: Option<u64>,
        span_ms: Option<u64>,
    ) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO listener_rejections \
                     (rejected_at, kind, passage_id, mbid, heard_ms, span_ms) \
                 VALUES (strftime('%s','now'), ?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![kind.as_str(), passage_id, mbid, heard_ms, span_ms],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// How many plays are on record. For diagnostics, and for anything that
    /// needs to see the ledger move without reading it all.
    pub fn play_count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM listener_play_history", [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// When each recording was last rejected in this way. Only the most recent
    /// matters: suppression is a window, not an accumulation.
    pub fn last_rejected(
        &self,
        kind: Rejection,
    ) -> Result<std::collections::HashMap<String, i64>, DbError> {
        let mut q = self
            .conn
            .prepare(
                "SELECT mbid, MAX(rejected_at) FROM listener_rejections                  WHERE mbid IS NOT NULL AND kind = ?1 GROUP BY mbid",
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        let rows = q
            .query_map([kind.as_str()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        Ok(rows.flatten().collect())
    }


    /// The saved resume point, or `None` on a first run.
    pub fn load(&self) -> Result<Option<(Option<i64>, u64, bool)>, DbError> {
        self.conn
            .query_row(
                "SELECT passage_id, position_ms, playing FROM player_state WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<i64>>(0)?,
                        r.get::<_, i64>(1)?.max(0) as u64,
                        r.get::<_, i64>(2)? != 0,
                    ))
                },
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(DbError::Query(other.to_string())),
            })
    }

}

/// Browsing the library, by artist, by album, by track `[REQ-VIS-180]`.
///
/// MuLibPlay's three "Browse by" pages, which are the one part of its interface
/// Vaino had no answer for. They group by the *displayed* name -- MusicBrainz
/// where it has one, the file's tag where it does not -- so what you can browse
/// by is exactly what you can see, rather than a second naming scheme that
/// disagrees with the player.
///
/// One shape underneath all three: every radio passage, the mbid that names it,
/// and the file whose tags stand in. Measured on this library, 463 artists in
/// 53 ms, 660 albums in 29 ms, 8,078 tracks in 80 ms -- on demand, not per tick,
/// so that is comfortably fast enough to leave as a query rather than a cache.
const NAMED: &str = "\
    SELECT p.passage_id, p.file_id, \
           (SELECT pr.mbid FROM passage_recordings pr \
             WHERE pr.passage_id = p.passage_id \
             ORDER BY pr.weight DESC, pr.mbid LIMIT 1) AS mbid \
      FROM passages p WHERE p.kind = 'radio'";

/// The displayed artist, as a SQL expression over `NAMED` joined to `file_tags`.
const ARTIST_EXPR: &str = "COALESCE( \
    (SELECT a.name FROM recording_artists ra JOIN artists a ON a.mbid = ra.artist_mbid \
      WHERE ra.mbid = m.mbid ORDER BY ra.weight DESC, a.name LIMIT 1), ft.artist)";

/// The displayed album: MusicBrainz **Release** title, then the file's tag.
const ALBUM_EXPR: &str = "COALESCE( \
    (SELECT rel.title FROM release_recordings rr JOIN releases rel ON rel.mbid = rr.release_mbid \
      WHERE rr.mbid = m.mbid ORDER BY rr.chosen DESC, rel.release_date, rel.title LIMIT 1), ft.album)";

const TITLE_EXPR: &str =
    "COALESCE((SELECT r.title FROM recordings r WHERE r.mbid = m.mbid), ft.title)";

const PLAYS_EXPR: &str =
    "(SELECT COUNT(*) FROM listener_play_history h WHERE h.mbid = m.mbid)";

/// The same naming as `TITLE_EXPR`/`ARTIST_EXPR`/`ALBUM_EXPR`, but without the
/// `file_tags` fallback `[REQ-VIS-250]`: history has no passage to join a file
/// through, and a rescan that renumbers passages must not blank out a title
/// six years old. `u` is the unioned history row, not `NAMED`'s `m`.
const HIST_TITLE_EXPR: &str = "(SELECT r.title FROM recordings r WHERE r.mbid = u.mbid)";
const HIST_ARTIST_EXPR: &str = "(SELECT a.name FROM recording_artists ra \
    JOIN artists a ON a.mbid = ra.artist_mbid \
    WHERE ra.mbid = u.mbid ORDER BY ra.weight DESC, a.name LIMIT 1)";
const HIST_ALBUM_EXPR: &str = "(SELECT rel.title FROM release_recordings rr \
    JOIN releases rel ON rel.mbid = rr.release_mbid \
    WHERE rr.mbid = u.mbid ORDER BY rr.chosen DESC, rel.release_date, rel.title LIMIT 1)";

#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowseGroup {
    pub name: String,
    /// The artist a release belongs to; `None` when browsing artists.
    pub artist: Option<String>,
    pub passages: i64,
    pub plays: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BrowseTrack {
    pub passage_id: i64,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub plays: i64,
    /// Position on the record, when the file knows it `[REQ-VIS-190]`.
    pub track_no: Option<i64>,
    pub disc_no: Option<i64>,
}

/// One row of the play-history page `[REQ-VIS-250]`: something that sounded
/// long enough to be counted, or long enough to be judged and declined.
///
/// Named by MusicBrainz alone, unlike [`BrowseTrack`] -- history has no
/// passage to fall back to a file's own tag with, and a play from six years
/// ago must still be nameable after the file that made it has gone.
#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    /// Unix seconds: when it played, or when it was skipped.
    pub at: i64,
    /// `"play"` if it crossed the threshold `[SPEC-PLAY-030]`, `"skip"` if it
    /// did not. Dequeues never sounded and do not appear here at all.
    pub kind: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// `None` for a row written before this column existed, or for a guest
    /// backend that never reported a span -- absent, not zero
    /// `[GOV-SRC-040]`.
    pub played_pct: Option<f64>,
    /// What a "flag this for review" checkbox on this row would set
    /// `[REQ-VIS-265]` -- `None` when neither a recording nor a live passage
    /// survives to flag: the file has since been relinked away and only the
    /// name persists `[SPEC-SC-095]`. The same `(subject_kind, subject_id)`
    /// shape `listener_flags` itself is keyed by.
    pub flag_kind: Option<&'static str>,
    pub flag_id: Option<String>,
    pub flagged: bool,
}

/// One candidate identity for a passage, as AcoustID reports it.
#[cfg(feature = "sampo-support")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Suggestion {
    pub mbid: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub score: f64,
}

/// A passage whose audio does not match the id it carries `[REQ-LIB-165]`.
#[cfg(feature = "sampo-support")]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReviewItem {
    pub passage_id: i64,
    pub stored_mbid: String,
    /// What the library currently believes, by the ordinary naming rules --
    /// which is what the listener sees, and so what is actually in question.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// AcoustID's best score for this audio, 0 to 1.
    pub score: Option<f64>,
    /// What it says the audio is instead, best first.
    pub suggested: Vec<Suggestion>,
    /// How wrong this looks, worst first. See `SEVERITIES`.
    pub severity: &'static str,
    /// Rank of `severity`, so the page can sort and group without keeping its
    /// own copy of the order.
    pub rank: u8,
    /// The judgement already recorded, if any. Present so a decision can be
    /// looked at again and withdrawn: a review tool whose every answer is
    /// final is one you have to be careful with rather than one you can think
    /// in `[REQ-LIB-165]`.
    pub decision: Option<String>,
    pub chosen_mbid: Option<String>,
    pub chosen_release_mbid: Option<String>,
    /// Set once `apply_reviews` has written the change into the library. A
    /// decision that has only been recorded can be withdrawn outright; one
    /// that has been applied has to be reverted, which is a different act and
    /// gets a different button.
    pub applied: bool,
    /// A recorded-but-not-necessarily-applied artist correction
    /// `[SPEC-SUI-197]`, independent of `decision` above: the recording can
    /// be exactly right while its credit is not, so this exists whether or
    /// not the recording itself was ever reassigned.
    pub artist_review: Option<String>,
    pub artist_review_applied: bool,
}

/// A release the chosen recording appears on, for naming the album
/// `[REQ-LIB-165]`.
#[cfg(feature = "sampo-support")]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReleaseOption {
    pub mbid: String,
    pub title: String,
    pub date: Option<String>,
    pub status: Option<String>,
    pub track_count: Option<i64>,
    /// Already the preferred one for this recording.
    pub chosen: bool,
}

/// How badly a stored id disagrees with the audio `[REQ-LIB-165]`.
///
/// A single "contradicted" flag is one bit, and one bit cannot tell a passage
/// playing under a completely wrong name from a remaster with its own MBID.
/// On this library that difference is 41 cases against 526, so it decides
/// whether the queue is worth opening.
///
/// The grades are the same distinctions `verify_ids.py` drew against the file
/// tags -- title agrees, artist agrees, neither -- applied here to evidence
/// that is actually independent.
#[cfg(feature = "sampo-support")]
pub const SEVERITIES: [(&str, u8, &str); 6] = [
    ("no-mbid", 0, "no MusicBrainz id at all -- a migration placeholder"),
    ("wrong-song", 1, "neither the title nor the performer matches"),
    ("wrong-artist", 2, "same title, different performer"),
    ("wrong-title", 3, "same performer, different title"),
    ("different-id", 4, "the same recording under another MBID"),
    ("unverified", 5, "AcoustID does not know this audio; not evidence"),
];

/// Does this even look like a MusicBrainz id?
///
/// The migration left 44 passages carrying `local:track:N`, which is not an
/// MBID and never was -- and two passages share `local:track:827`, so they do
/// not even identify a track uniquely. Everything downstream keys on this
/// string: play history, rotation, naming. A passage carrying one is not a
/// *questionable* identification, it is an absent one.
///
/// Shape-checked rather than prefix-checked, so any other non-conforming id
/// the migration produced is caught too, not just the one spelling of it.
#[cfg(feature = "sampo-support")]
pub fn is_mbid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Grade one disagreement.
///
/// Absent evidence is never treated as agreement -- a suggestion whose artist
/// disagrees is a real finding. But a field nobody has an opinion about is not
/// a disagreement either: if the library holds no artist for the passage, or
/// no candidate names one, then the artist cannot be *wrong*, and grading it
/// `wrong-artist` would invent a dispute out of two silences. In that case the
/// title decides alone.
#[cfg(feature = "sampo-support")]
fn grade(
    stored_mbid: &str,
    title: Option<&str>,
    artist: Option<&str>,
    suggested: &[Suggestion],
) -> (&'static str, u8) {
    // Checked before anything else, and regardless of what the audio says: a
    // passage with no real id is broken whether or not AcoustID recognises it,
    // and it cannot be "the same recording under another MBID" when it has
    // none. These lead the queue because they are certain rather than merely
    // likely -- and because a fingerprint match gives them their first real id.
    if !is_mbid(stored_mbid) {
        return ("no-mbid", 0);
    }
    if suggested.is_empty() {
        return ("unverified", 5);
    }
    let title_ok = title.is_some_and(|t| {
        suggested.iter().any(|s| s.title.as_deref().is_some_and(|x| same_title(t, x)))
    });
    let comparable = artist.is_some() && suggested.iter().any(|s| s.artist.is_some());
    if !comparable {
        return if title_ok { ("different-id", 4) } else { ("wrong-song", 1) };
    }
    let artist_ok = artist.is_some_and(|a| {
        suggested.iter().any(|s| s.artist.as_deref().is_some_and(|x| same_title(a, x)))
    });
    match (title_ok, artist_ok) {
        (true, true) => ("different-id", 4),
        (true, false) => ("wrong-artist", 2),
        (false, true) => ("wrong-title", 3),
        (false, false) => ("wrong-song", 1),
    }
}

/// Strip what differs between two spellings of one title without changing
/// which song it is: bracketed qualifiers, punctuation, case, leading article.
///
/// Deliberately blunt. It decides how a row is *labelled and ordered*, never
/// whether anything is changed, so a wrong answer costs a misfiled card.
#[cfg(feature = "sampo-support")]
fn same_title(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        let mut out = String::new();
        let mut depth = 0usize;
        for ch in s.chars() {
            match ch {
                '(' | '[' => depth += 1,
                ')' | ']' => depth = depth.saturating_sub(1),
                _ if depth == 0 => out.push(ch.to_ascii_lowercase()),
                _ => {}
            }
        }
        let cleaned: String = out
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { ' ' })
            .collect();
        let t = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        for article in ["the ", "a ", "an "] {
            if let Some(rest) = t.strip_prefix(article) {
                return rest.to_string();
            }
        }
        t
    }
    let (x, y) = (norm(a), norm(b));
    !x.is_empty() && x == y
}

#[cfg(feature = "sampo-support")]
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ReviewProgress {
    /// False when the fingerprint pass has never been run and merged. "No
    /// findings" and "never looked" must not render the same.
    pub ran: bool,
    pub checked: i64,
    pub confirmed: i64,
    pub contradicted: i64,
    pub decided: i64,
}

/// A recorded-but-not-yet-applied boundary edit `[SPEC021 §2]`.
#[cfg(feature = "sampo-support")]
#[derive(Debug, Clone, serde::Serialize)]
pub struct BoundaryReview {
    pub start_ms: u64,
    pub end_ms: u64,
    pub lead_in_ms: Option<u64>,
    pub lead_out_ms: Option<u64>,
    pub gain_db: Option<f64>,
}

/// What to narrow a browse to. Every field is a whole-value match except `q`,
/// which is a substring -- the difference between "this artist" and "anything
/// that looks like this".
#[derive(Debug, Default, Clone)]
pub struct BrowseFilter {
    pub q: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl BrowseFilter {
    fn like(&self) -> String {
        self.q.as_deref().map(|s| format!("%{s}%")).unwrap_or_default()
    }
}

impl Library {
    pub fn browse_artists(&self, f: &BrowseFilter) -> Result<Vec<BrowseGroup>, DbError> {
        let sql = format!(
            "SELECT artist, COUNT(*), SUM(plays) FROM ( \
               SELECT {ARTIST_EXPR} AS artist, {PLAYS_EXPR} AS plays \
                 FROM ({NAMED}) m LEFT JOIN file_tags ft ON ft.file_id = m.file_id) \
             WHERE artist IS NOT NULL AND artist <> '' \
               AND (?1 = '' OR artist LIKE ?1) \
             GROUP BY artist ORDER BY artist COLLATE NOCASE"
        );
        self.groups(&sql, rusqlite::params![f.like()], false)
    }

    pub fn browse_albums(&self, f: &BrowseFilter) -> Result<Vec<BrowseGroup>, DbError> {
        let sql = format!(
            "SELECT album, COUNT(*), SUM(plays), artist FROM ( \
               SELECT {ALBUM_EXPR} AS album, {ARTIST_EXPR} AS artist, {PLAYS_EXPR} AS plays \
                 FROM ({NAMED}) m LEFT JOIN file_tags ft ON ft.file_id = m.file_id) \
             WHERE album IS NOT NULL AND album <> '' \
               AND (?1 = '' OR album LIKE ?1) \
               AND (?2 = '' OR artist = ?2) \
             GROUP BY album ORDER BY album COLLATE NOCASE"
        );
        let artist = f.artist.clone().unwrap_or_default();
        self.groups(&sql, rusqlite::params![f.like(), artist], true)
    }

    fn groups(
        &self,
        sql: &str,
        params: impl rusqlite::Params,
        with_artist: bool,
    ) -> Result<Vec<BrowseGroup>, DbError> {
        let mut st = self.conn.prepare(sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map(params, |r| {
                Ok(BrowseGroup {
                    name: r.get(0)?,
                    passages: r.get(1)?,
                    plays: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    artist: if with_artist { r.get(3)? } else { None },
                })
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Passages the audio disagrees with, oldest check first `[REQ-LIB-165]`.
    ///
    /// Only `contradicted`: a fingerprint that matched something else with high
    /// confidence. `unmatched` is not evidence of anything -- AcoustID simply
    /// has no entry -- and putting those in front of a person would bury the
    /// real findings under thousands of non-findings.
    ///
    /// Already-decided passages are excluded, so the queue empties as it is
    /// worked through rather than re-presenting settled questions.
    #[cfg(feature = "sampo-support")]
    pub fn review_queue(&self, limit: usize) -> Result<Vec<ReviewItem>, DbError> {
        // `id_checks` is written by the fingerprint pass, not by the player, so
        // on a library where that has never been run the table is simply absent
        // -- and a query naming a missing table FAILS rather than returning
        // nothing. That exact mistake blanked the browse page twice. Nothing to
        // review is a legitimate state and must not look like a broken page.
        // A locally-ingested id is excluded only when AcoustID also drew a
        // blank. Both halves matter:
        //
        // * `local:ingest` + `unmatched` is self-published music -- nothing
        //   can name it, and asking a person would be an unanswerable question
        //   parked at the top of the queue for ever.
        // * `local:ingest` + `contradicted` is a commercial album ingested
        //   from a folder, where AcoustID *does* know what it is. That is the
        //   most useful row in the queue: a placeholder with the real
        //   recording sitting beside it, ready to accept.
        //
        // The migration's `local:track:N` carries `inherited:mulib`, so it is
        // never excluded on either count -- it really is a broken id.
        //
        // `id_reviews` is checked too: it is created by `PlayerStore::open`,
        // which any running server has done, but this handle does not itself
        // guarantee it.
        if !self.has_table("id_checks") || !self.has_table("id_reviews")
            || !self.has_table("artist_reviews")
        {
            return Ok(Vec::new());
        }
        // Decided passages come back too, carrying their judgement, so that a
        // decision can be found again and withdrawn. They are a separate grade
        // on the page and switched off by default, so working through the
        // queue still shortens it.
        // `artist_reviews` is joined here too, though it corrects a table
        // `id_checks`/`id_reviews` never touch `[SPEC-SUI-197]` -- reachability
        // for this correction rides on whatever else put the passage in front
        // of a person, since nothing about a right recording's wrong credit
        // makes AcoustID disagree with anything. Joined by `m.mbid` -- the
        // passage's CURRENT recording link, which `NAMED` already computes --
        // not by passage, since the table is keyed by recording.
        let sql = format!(
            "SELECT c.passage_id, c.stored_mbid, c.score, c.suggested, \
                    {TITLE_EXPR}, {ARTIST_EXPR}, {ALBUM_EXPR}, \
                    v.decision, v.chosen_mbid, v.chosen_release_mbid, v.applied_at, \
                    a.artist_name, a.applied_at \
               FROM id_checks c \
               JOIN ({NAMED}) m ON m.passage_id = c.passage_id \
               LEFT JOIN file_tags ft ON ft.file_id = m.file_id \
               LEFT JOIN id_reviews v ON v.passage_id = c.passage_id \
               LEFT JOIN artist_reviews a ON a.recording_mbid = m.mbid \
              WHERE c.verdict IN ('contradicted', 'unmatched') \
                AND NOT (c.verdict = 'unmatched' \
                         AND EXISTS (SELECT 1 FROM passage_recordings pr \
                                      WHERE pr.passage_id = c.passage_id \
                                        AND pr.source = 'local:ingest')) \
              ORDER BY c.score DESC, c.passage_id LIMIT ?1"
        );
        let mut st = self.conn.prepare(&sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map([limit as i64], |r| {
                let raw: Option<String> = r.get(3)?;
                // A malformed payload becomes an empty list rather than an
                // error: the row is still worth showing, minus its options.
                let suggested: Vec<Suggestion> = raw
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
                let title: Option<String> = r.get(4)?;
                let artist: Option<String> = r.get(5)?;
                let stored_mbid: String = r.get(1)?;
                let (severity, rank) =
                    grade(&stored_mbid, title.as_deref(), artist.as_deref(), &suggested);
                let applied_at: Option<String> = r.get(10)?;
                let artist_review: Option<String> = r.get(11)?;
                let artist_review_applied_at: Option<String> = r.get(12)?;
                Ok(ReviewItem {
                    passage_id: r.get(0)?,
                    stored_mbid,
                    score: r.get(2)?,
                    suggested,
                    title,
                    artist,
                    album: r.get(6)?,
                    severity,
                    rank,
                    decision: r.get(7)?,
                    chosen_mbid: r.get(8)?,
                    chosen_release_mbid: r.get(9)?,
                    applied: applied_at.is_some(),
                    artist_review,
                    artist_review_applied: artist_review_applied_at.is_some(),
                })
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        let mut items: Vec<ReviewItem> = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))?;
        // Worst first. Within a grade the strongest match leads, which is the
        // order the query already produced -- `sort_by_key` is stable, so that
        // ordering survives.
        items.sort_by_key(|i| i.rank);
        Ok(items)
    }

    /// Releases this recording appears on, for choosing which album to call it
    /// `[REQ-LIB-165]`.
    ///
    /// A recording is on many releases -- the album, the remaster, three
    /// compilations -- and `ALBUM_EXPR` picks by `chosen DESC` then date. That
    /// resolves ties by age, which is a guess. This lets the answer be stated.
    ///
    /// Only releases Sampo has already fetched can be offered. A recording new
    /// to the library has none, and the album then falls back to the file's own
    /// tag, which is the designed fallback rather than a failure.
    #[cfg(feature = "sampo-support")]
    pub fn releases_for(&self, recording_mbid: &str) -> Result<Vec<ReleaseOption>, DbError> {
        if !self.has_table("release_recordings") {
            return Ok(Vec::new());
        }
        let mut st = self
            .conn
            .prepare(
                "SELECT rel.mbid, rel.title, rel.release_date, rel.status, \
                        rel.track_count, COALESCE(rr.chosen, 0) \
                   FROM release_recordings rr \
                   JOIN releases rel ON rel.mbid = rr.release_mbid \
                  WHERE rr.mbid = ?1 \
                  ORDER BY rr.chosen DESC, rel.release_date, rel.title",
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map([recording_mbid], |r| {
                Ok(ReleaseOption {
                    mbid: r.get(0)?,
                    title: r.get(1)?,
                    date: r.get(2)?,
                    status: r.get(3)?,
                    track_count: r.get(4)?,
                    chosen: r.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// The stored cover for a passage's recording `[REQ-VIS-170]`.
    ///
    /// Looked up through the release Sampo chose when that release has the
    /// art; otherwise through any other release known to carry the same
    /// recording. Sampo's pick and a hand-curated pick (MuLibPlay's, notably
    /// `[GDE-PHS-010]`) often name different pressings of the same release,
    /// and a recording without art is worse than one shown under a release
    /// that is not quite the chosen edition -- so `covers.rs` already takes
    /// this same fallback for the MPD-facing cover file, and this matches it.
    /// Absent table, absent row and a blob too small to be a picture all mean
    /// the same thing to the caller: no art, show nothing.
    pub fn stored_art(&self, passage_id: i64, back: bool) -> Option<crate::tags::Artwork> {
        if !self.has_table("cover_art") {
            return None;
        }
        let col = if back { "back" } else { "front" };
        let data: Vec<u8> = self
            .conn
            .query_row(
                &format!(
                    "SELECT a.{col} FROM cover_art a \
                       JOIN release_recordings rr ON rr.release_mbid = a.release_mbid \
                       JOIN passage_recordings pr ON pr.mbid = rr.mbid \
                      WHERE pr.passage_id = ?1 AND a.{col} IS NOT NULL \
                      ORDER BY rr.chosen DESC LIMIT 1"
                ),
                [passage_id],
                |r| r.get(0),
            )
            .ok()?;
        if data.len() < crate::tags::MIN_ART_BYTES {
            return None;
        }
        // Sniffed rather than stored: the archive serves JPEG and PNG, and a
        // wrong Content-Type would render as a broken image.
        let media_type = if data.starts_with(&[0x89, b'P', b'N', b'G']) {
            "image/png"
        } else {
            "image/jpeg"
        };
        Some(crate::tags::Artwork { media_type: media_type.into(), data })
    }

    /// How much reviewing there is to do, and how much has been done.
    ///
    /// Returned even when `id_checks` has never been created -- the pass may
    /// simply not have been run -- because "no findings" and "never looked"
    /// are different states and the page says which one it is in.
    #[cfg(feature = "sampo-support")]
    pub fn review_progress(&self) -> ReviewProgress {
        let n = |sql: &str| -> i64 { self.conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        ReviewProgress {
            ran: self.has_table("id_checks"),
            checked: n("SELECT COUNT(*) FROM id_checks"),
            contradicted: n("SELECT COUNT(*) FROM id_checks WHERE verdict = 'contradicted'"),
            confirmed: n("SELECT COUNT(*) FROM id_checks WHERE verdict = 'confirmed'"),
            decided: n("SELECT COUNT(*) FROM id_reviews"),
        }
    }

    /// A recorded-but-not-yet-applied boundary edit, if there is one
    /// `[SPEC021 §2]`. Reopening the editor after a commit must show the
    /// edit that was made, not the stale automatic values it drafted over --
    /// so `/edit/:id/info` prefers this when it exists.
    ///
    /// `boundary_reviews` is created by `PlayerStore::open`, but this handle
    /// does not itself guarantee that has run -- guarded the same way
    /// `review_queue` guards `id_checks`, since a query naming a table that
    /// does not exist fails outright rather than finding nothing.
    #[cfg(feature = "sampo-support")]
    pub fn boundary_review(&self, passage_id: i64) -> Option<BoundaryReview> {
        if !self.has_table("boundary_reviews") {
            return None;
        }
        self.conn
            .query_row(
                "SELECT start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db
                   FROM boundary_reviews WHERE passage_id = ?1",
                [passage_id],
                |r| {
                    Ok(BoundaryReview {
                        start_ms: r.get::<_, i64>(0)? as u64,
                        end_ms: r.get::<_, i64>(1)? as u64,
                        lead_in_ms: r.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                        lead_out_ms: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
                        gain_db: r.get(4)?,
                    })
                },
            )
            .ok()
    }

    fn has_table(&self, name: &str) -> bool {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [name],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0
    }

    pub fn browse_tracks(&self, f: &BrowseFilter) -> Result<Vec<BrowseTrack>, DbError> {
        // An album is a running order, not an index: opened as one, its tracks
        // belong in the order they were put on the record `[REQ-VIS-190]`.
        // Anywhere else alphabetical is what makes a long list findable.
        // Unnumbered tracks sort after the numbered ones rather than ahead of
        // them, which is where a bare NULL would put them.
        let order = if f.album.is_some() {
            // MusicBrainz first, the file's tag second `[REQ-VIS-190]`. Both are
            // track numbers; one is the release's own and the other is whatever
            // the person who ripped the disc typed, so when Sampo has chosen a
            // release its numbering wins. Unnumbered tracks still sort last.
            "ORDER BY COALESCE(mb_disc, disc_no, 1), \
                      CASE WHEN COALESCE(mb_track, track_no) IS NULL THEN 1 ELSE 0 END, \
                      COALESCE(mb_track, track_no), \
                      title COLLATE NOCASE"
        } else {
            "ORDER BY title COLLATE NOCASE"
        };
        let sql = format!(
            "SELECT passage_id, title, artist, album, plays, \
                    COALESCE(mb_track, track_no), COALESCE(mb_disc, disc_no) FROM ( \
               SELECT m.passage_id, {TITLE_EXPR} AS title, {ARTIST_EXPR} AS artist, \
                      {ALBUM_EXPR} AS album, {PLAYS_EXPR} AS plays, \
                      ft.track_no AS track_no, ft.disc_no AS disc_no, \
                      (SELECT rr.position FROM release_recordings rr \
                        WHERE rr.mbid = m.mbid AND rr.chosen = 1) AS mb_track, \
                      (SELECT rr.disc FROM release_recordings rr \
                        WHERE rr.mbid = m.mbid AND rr.chosen = 1) AS mb_disc \
                 FROM ({NAMED}) m LEFT JOIN file_tags ft ON ft.file_id = m.file_id) \
             WHERE title IS NOT NULL AND title <> '' \
               AND (?1 = '' OR title LIKE ?1) \
               AND (?2 = '' OR artist = ?2) \
               AND (?3 = '' OR album = ?3) \
             {order} LIMIT {limit}",
            limit = crate::BROWSE_LIMIT
        );
        let mut st = self.conn.prepare(&sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map(
                rusqlite::params![
                    f.like(),
                    f.artist.clone().unwrap_or_default(),
                    f.album.clone().unwrap_or_default()
                ],
                |r| {
                    Ok(BrowseTrack {
                        passage_id: r.get(0)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        album: r.get(3)?,
                        plays: r.get(4)?,
                        track_no: r.get(5)?,
                        disc_no: r.get(6)?,
                    })
                },
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// A page of what has actually sounded, newest first `[REQ-VIS-250]`.
    ///
    /// Plays and skips, unioned into one timeline: both started sounding, and
    /// what tells them apart -- did it reach the threshold -- is exactly the
    /// question this page answers per row. A dequeue never sounded and has no
    /// place in a *play* history.
    pub fn play_history(&self, limit: i64, offset: i64) -> Result<Vec<HistoryEntry>, DbError> {
        // `subject_kind`/`subject_id` resolve to a recording when the row has
        // an mbid (survives a rescan `[SPEC-DF-035]`, same reason the naming
        // columns are mbid-first), else to the passage -- often exactly the
        // unidentified case someone most wants to flag. `passage_id` here is
        // never a stale one: `ON DELETE SET NULL` already blanks it the
        // moment the passage it named stops existing.
        let sql = format!(
            "SELECT at, kind, heard_ms, span_ms, \
                    {HIST_TITLE_EXPR} AS title, {HIST_ARTIST_EXPR} AS artist, \
                    {HIST_ALBUM_EXPR} AS album, \
                    CASE WHEN u.mbid IS NOT NULL THEN 'recording' \
                         WHEN u.passage_id IS NOT NULL THEN 'passage' END AS subject_kind, \
                    COALESCE(u.mbid, CAST(u.passage_id AS TEXT)) AS subject_id, \
                    EXISTS (SELECT 1 FROM listener_flags f \
                             WHERE f.subject_kind = CASE WHEN u.mbid IS NOT NULL THEN 'recording' \
                                                          ELSE 'passage' END \
                               AND f.subject_id = COALESCE(u.mbid, CAST(u.passage_id AS TEXT))) AS flagged \
               FROM (SELECT played_at AS at, 'play' AS kind, mbid, passage_id, heard_ms, span_ms \
                       FROM listener_play_history \
                     UNION ALL \
                     SELECT rejected_at AS at, 'skip' AS kind, mbid, passage_id, heard_ms, span_ms \
                       FROM listener_rejections WHERE kind = 'skip') u \
              ORDER BY at DESC LIMIT ?1 OFFSET ?2"
        );
        let mut st = self.conn.prepare(&sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = st
            .query_map(rusqlite::params![limit, offset], |r| {
                let heard_ms: Option<i64> = r.get(2)?;
                let span_ms: Option<i64> = r.get(3)?;
                let played_pct = match (heard_ms, span_ms) {
                    (Some(h), Some(s)) if s > 0 => {
                        Some((h as f64 / s as f64 * 100.0).clamp(0.0, 100.0))
                    }
                    _ => None,
                };
                let subject_kind: Option<String> = r.get(7)?;
                Ok(HistoryEntry {
                    at: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(4)?,
                    artist: r.get(5)?,
                    album: r.get(6)?,
                    played_pct,
                    flag_kind: match subject_kind.as_deref() {
                        Some("recording") => Some("recording"),
                        Some("passage") => Some("passage"),
                        _ => None,
                    },
                    flag_id: r.get(8)?,
                    flagged: r.get(9)?,
                })
            })
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// How many rows [`play_history`](Self::play_history) has to page through,
    /// so the page can say "page 3 of 41" rather than guessing when to stop
    /// offering "next".
    pub fn play_history_count(&self) -> Result<i64, DbError> {
        self.conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM listener_play_history) + \
                        (SELECT COUNT(*) FROM listener_rejections WHERE kind = 'skip')",
                [],
                |r| r.get(0),
            )
            .map_err(|e| DbError::Query(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the minimum of SPEC008 the player touches, so these tests pin the
    /// column names the queries depend on.
    fn fixture() -> Connection {
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
                                                   (2,'aaaaaaaa-0000-0000-0000-000000000001',0.9,'s');",
        )
        .unwrap();
        c
    }

    /// The review fixture: the naming tables the queue joins, plus findings.
    fn reviewable() -> Connection {
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

    /// Confirmed ids never reach a person: 6,591 of them here, and there is
    /// nothing to decide about a passage the audio agrees with. What does
    /// reach the page arrives with the candidates that dispute it.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn settled_ids_stay_out_of_the_queue() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (3,1,'radio',0,1000,NULL,NULL,NULL,'src');
             INSERT INTO passage_recordings VALUES (3,'aaaaaaaa-0000-0000-0000-000000000005',1.0,'s');
             INSERT INTO id_checks VALUES (3,'aaaaaaaa-0000-0000-0000-000000000005','confirmed',0.99,NULL,'t');",
        )
        .unwrap();
        let lib = Library { conn: c };
        let q = lib.review_queue(50).unwrap();
        assert_eq!(q.len(), 1, "a confirmed id is not a question");
        assert_eq!(q[0].passage_id, 2);
        assert_eq!(q[0].suggested.len(), 1);
        assert_eq!(q[0].suggested[0].mbid, "aaaaaaaa-0000-0000-0000-000000000002");
        assert_eq!(q[0].suggested[0].title.as_deref(), Some("Right Song"));
    }

    /// Most contradictions on this library are the same song under a different
    /// recording id -- another pressing, a remaster, a 5.1 mix. That is a much
    /// smaller problem than a passage playing under the wrong name, and the
    /// queue leads with the ones where the names disagree too.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_different_pressing_is_told_apart_from_a_wrong_song() {
        assert!(same_title("Why Worry", "Why Worry (5.1 mix)"));
        assert!(same_title("Rock 'n' Roll Suicide", "Rock ’n’ Roll Suicide"));
        assert!(same_title("There Must Be an Angel (Playing With My Heart)",
                           "There Must Be an Angel (long version)"));
        assert!(same_title("The Chain", "Chain"), "a leading article is not a song");
        assert!(!same_title("Take My Breath Away", "S.M.D.U."));
        assert!(!same_title("", "Anything"), "an empty title matches nothing");

        let c = reviewable();
        // A second passage whose audio is a different song entirely. It is the
        // weaker match of the two, so if it still comes first the ordering is
        // by kind and not merely by score.
        c.execute_batch(
            "INSERT INTO passages VALUES (4,1,'radio',0,1000,NULL,NULL,NULL,'src');
             INSERT INTO passage_recordings VALUES (4,'aaaaaaaa-0000-0000-0000-000000000003',1.0,'s');
             INSERT INTO recordings VALUES ('aaaaaaaa-0000-0000-0000-000000000003','Wrong Song',NULL,'s');
             INSERT INTO id_checks VALUES (4,'aaaaaaaa-0000-0000-0000-000000000003','contradicted',0.91,
                 '[{\"mbid\":\"rec-q\",\"title\":\"Wrong Song (remaster)\",\"score\":0.91}]','t');",
        )
        .unwrap();
        let lib = Library { conn: c };
        let q = lib.review_queue(50).unwrap();
        assert_eq!(q.len(), 2);
        assert_eq!(q[0].passage_id, 2, "the worse grade leads, despite a lower score");
        assert_eq!(q[0].severity, "wrong-song");
        assert_eq!(q[1].severity, "different-id",
                   "a remaster of the same title is not a wrong song");
        assert!(q[0].rank < q[1].rank);
    }

    /// Severity is what makes the queue usable: 41 cases against 526 on this
    /// library, and one bit cannot tell them apart. Absent evidence must never
    /// count as agreement -- a suggestion with no artist cannot confirm one.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_disagreement_is_graded_by_how_much_disagrees() {
        let s = |t: Option<&str>, a: Option<&str>| Suggestion {
            mbid: "x".into(),
            title: t.map(str::to_string),
            artist: a.map(str::to_string),
            score: 0.9,
        };
        let real = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let g = |t, a, sug: &[Suggestion]| grade(real, t, a, sug).0;

        assert_eq!(g(Some("Why Worry"), Some("Dire Straits"),
                     &[s(Some("Why Worry (5.1 mix)"), Some("Dire Straits"))]),
                   "different-id");
        assert_eq!(g(Some("Alice the Camel"), Some("Baby Reflections"),
                     &[s(Some("Alice the Camel"), Some("Kimmy / Steve"))]),
                   "wrong-artist");
        assert_eq!(g(Some("Take My Breath Away"), Some("Berlin"),
                     &[s(Some("S.M.D.U."), Some("Berlin"))]),
                   "wrong-title");
        assert_eq!(g(Some("Take My Breath Away"), Some("Berlin"),
                     &[s(Some("S.M.D.U."), Some("Brock Landars"))]),
                   "wrong-song");
        assert_eq!(g(Some("Anything"), Some("Anyone"), &[]), "unverified",
                   "no match at all is not evidence of a wrong song");
        // Two silences are not a disagreement. When no candidate states an
        // artist there is nothing to dispute, so the title decides alone --
        // grading this `wrong-artist` would invent a case for someone to
        // adjudicate out of missing data.
        assert_eq!(g(Some("Why Worry"), Some("Dire Straits"),
                     &[s(Some("Why Worry"), None)]),
                   "different-id");
        assert_eq!(g(Some("Why Worry"), None,
                     &[s(Some("Why Worry"), Some("Dire Straits"))]),
                   "different-id", "a passage with no artist has none to be wrong");
        // But a candidate that names a DIFFERENT artist is a real finding, and
        // must not be softened by a second candidate that names none.
        assert_eq!(g(Some("Why Worry"), Some("Dire Straits"),
                     &[s(Some("Why Worry"), Some("Someone Else")), s(Some("Why Worry"), None)]),
                   "wrong-artist");
        // And the grades stay in step with the table the page reads.
        for (name, rank, _) in SEVERITIES {
            assert!(rank < 6, "{name} has no place in the order");
        }
    }

    /// Art is looked up through the **release**, so a folder holding two albums
    /// gives each its own cover -- which is the case `folder.jpg` cannot serve,
    /// and the one the DAO rips in this library actually present.
    #[test]
    fn stored_art_is_found_through_the_chosen_release() {
        let c = reviewable();
        let big = vec![0xFFu8; 512];          // large enough to be a picture
        c.execute_batch(ART_TABLE).unwrap();
        c.execute(
            "INSERT INTO releases (mbid,title,source) VALUES ('rel-1','A Record','mb')", [])
            .unwrap();
        c.execute(
            "INSERT INTO release_recordings (release_mbid,mbid,source,chosen) \
             VALUES ('rel-1','aaaaaaaa-0000-0000-0000-000000000001','mb',1)", [])
            .unwrap();
        // A PNG magic number in front, so the sniffer has something to find.
        let mut png = vec![0x89u8, b'P', b'N', b'G'];
        png.extend(std::iter::repeat_n(0u8, 600));
        c.execute("INSERT INTO cover_art VALUES ('rel-1',?1,?2,'test','t')",
                  rusqlite::params![big.clone(), png])
            .unwrap();
        let lib = Library { conn: c };
        let front = lib.stored_art(2, false).expect("front cover");
        assert_eq!(front.media_type, "image/jpeg");
        assert_eq!(front.data.len(), 512);
        assert!(lib.stored_art(2, true).is_some(), "the back is stored too");
        assert!(lib.stored_art(999, false).is_none(), "unknown passage, no art");
    }

    /// Sampo's chosen release and a hand-curated pick (MuLibPlay's) often name
    /// different pressings of the same recording. When the chosen release has
    /// no art, a non-chosen release of the same recording is still offered --
    /// matching the fallback `covers.rs` already takes for the MPD-facing
    /// cover file -- rather than showing nothing over a technicality.
    #[test]
    fn stored_art_falls_back_to_a_non_chosen_release() {
        let c = reviewable();
        let big = vec![0xFFu8; 512];
        c.execute_batch(ART_TABLE).unwrap();
        c.execute(
            "INSERT INTO releases (mbid,title,source) VALUES \
             ('rel-chosen','Sampo''s Pick','mb'), ('rel-other','MuLibPlay''s Pick','mb')", [])
            .unwrap();
        c.execute(
            "INSERT INTO release_recordings (release_mbid,mbid,source,chosen) VALUES \
             ('rel-chosen','aaaaaaaa-0000-0000-0000-000000000001','mb',1), \
             ('rel-other','aaaaaaaa-0000-0000-0000-000000000001','mb',0)", [])
            .unwrap();
        // Only the non-chosen release carries art.
        c.execute("INSERT INTO cover_art VALUES ('rel-other',?1,NULL,'test','t')",
                  rusqlite::params![big.clone()])
            .unwrap();
        let lib = Library { conn: c };
        let front = lib.stored_art(2, false).expect("found through the other release");
        assert_eq!(front.data.len(), 512);
    }

    /// A blob too small to be a picture is not a picture. MuLibPlay applied the
    /// same floor: a truncated download would otherwise render as a broken
    /// image, which reads as a fault in the player rather than a gap in data.
    #[test]
    fn a_blob_too_small_to_be_a_picture_is_not_offered() {
        let c = reviewable();
        c.execute_batch(ART_TABLE).unwrap();
        c.execute(
            "INSERT INTO releases (mbid,title,source) VALUES ('rel-1','A Record','mb')", [])
            .unwrap();
        c.execute(
            "INSERT INTO release_recordings (release_mbid,mbid,source,chosen) \
             VALUES ('rel-1','aaaaaaaa-0000-0000-0000-000000000001','mb',1)", [])
            .unwrap();
        c.execute("INSERT INTO cover_art VALUES ('rel-1',?1,NULL,'test','t')",
                  rusqlite::params![vec![0u8; crate::tags::MIN_ART_BYTES - 1]])
            .unwrap();
        let lib = Library { conn: c };
        assert!(lib.stored_art(2, false).is_none());
    }

    /// A library with no `cover_art` table at all -- one Sampo has never
    /// fetched for -- must answer "no art", not fail the request.
    #[test]
    fn a_library_without_the_art_table_simply_has_no_art() {
        let lib = Library { conn: fixture() };
        assert!(lib.stored_art(2, false).is_none());
        assert!(lib.stored_art(2, true).is_none());
    }

    /// A passage with no real id is not a *questionable* identification, it is
    /// an absent one. The migration left 44 of them carrying `local:track:N`,
    /// two of which share a number, so they do not even identify a track.
    /// Everything downstream keys on this string.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_passage_with_no_mbid_leads_the_queue() {
        assert!(is_mbid("68684e6b-37d2-487e-8ee2-d21e28fa1589"));
        assert!(!is_mbid("local:track:827"));
        assert!(!is_mbid(""));
        assert!(!is_mbid("68684e6b37d2487e8ee2d21e28fa1589"), "no dashes is not an MBID");
        assert!(!is_mbid("68684e6b-37d2-487e-8ee2-d21e28fa158g"), "g is not hex");
        assert!(!is_mbid("68684e6b-37d2-487e-8ee2-d21e28fa1589x"), "too long");

        // Graded before the audio is consulted at all: a placeholder is broken
        // whether or not AcoustID recognises the sound, and cannot be "the
        // same recording under another MBID" when it has no id to differ from.
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (6,1,'radio',0,1000,NULL,NULL,NULL,'src');
             INSERT INTO recordings VALUES ('local:track:827','Some Track',NULL,'s');
             INSERT INTO passage_recordings VALUES (6,'local:track:827',1.0,'s');
             INSERT INTO id_checks VALUES (6,'local:track:827','unmatched',NULL,NULL,'t');",
        )
        .unwrap();
        let lib = Library { conn: c };
        let q = lib.review_queue(50).unwrap();
        assert_eq!(q[0].passage_id, 6, "a missing id outranks every wrong one");
        assert_eq!(q[0].severity, "no-mbid");
        assert_eq!(q[0].rank, 0);
    }

    /// Music that has no MusicBrainz entry is not a fault to be reviewed.
    /// Self-published audio ingested from a folder carries a local id on
    /// purpose; asking a person about it would put an unanswerable question at
    /// the top of the queue for ever. The migration's placeholders stay,
    /// because those really are broken identifications -- the difference is
    /// the source, not the shape of the id.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_deliberately_local_id_is_not_a_review_finding() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (7,1,'radio',0,1000,NULL,NULL,NULL,'ingest:whole-file');
             INSERT INTO recordings VALUES ('local:audio:abc','My Own Song',NULL,'local:ingest');
             INSERT INTO passage_recordings VALUES (7,'local:audio:abc',1.0,'local:ingest');
             INSERT INTO id_checks VALUES (7,'local:audio:abc','unmatched',NULL,NULL,'t');
             -- and a migration placeholder, which must still be asked about
             INSERT INTO passages VALUES (8,1,'radio',0,1000,NULL,NULL,NULL,'src');
             INSERT INTO recordings VALUES ('local:track:827','Something',NULL,'s');
             INSERT INTO passage_recordings VALUES (8,'local:track:827',1.0,'inherited:mulib');
             INSERT INTO id_checks VALUES (8,'local:track:827','unmatched',NULL,NULL,'t');",
        )
        .unwrap();
        let lib = Library { conn: c };
        let q = lib.review_queue(50).unwrap();
        let ids: Vec<i64> = q.iter().map(|i| i.passage_id).collect();
        assert!(!ids.contains(&7),
                "self-published + unmatched: nothing can name it, so do not ask");
        assert!(ids.contains(&8), "a migration placeholder must still be queued");

        // But a locally-ingested track AcoustID *can* name is the most useful
        // row there is: a placeholder with the real recording beside it.
        lib.conn.execute_batch(
            "INSERT INTO passages VALUES (9,1,'radio',0,1000,NULL,NULL,NULL,'ingest:whole-file');
             INSERT INTO recordings VALUES ('local:audio:def','Some Album Track',NULL,'local:ingest');
             INSERT INTO passage_recordings VALUES (9,'local:audio:def',1.0,'local:ingest');
             INSERT INTO id_checks VALUES (9,'local:audio:def','contradicted',0.98,
                 '[{\"mbid\":\"aaaaaaaa-0000-0000-0000-00000000000c\",\"title\":\"Some Album Track\",\"score\":0.98}]','t');")
            .unwrap();
        let q2 = lib.review_queue(50).unwrap();
        let ids2: Vec<i64> = q2.iter().map(|i| i.passage_id).collect();
        assert!(ids2.contains(&9),
                "a local id AcoustID can name must be offered for review");
        assert_eq!(
            q.iter().find(|i| i.passage_id == 8).unwrap().severity,
            "no-mbid"
        );
    }

    /// `unmatched` reaches the page so it can be asked for deliberately, but
    /// it is graded lowest and the page leaves it off by default. It is 864
    /// passages here, and defaulting it on would bury the 41 that matter.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn unmatched_is_reachable_but_graded_lowest() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (5,1,'radio',0,1000,NULL,NULL,NULL,'src');
             INSERT INTO passage_recordings VALUES (5,'aaaaaaaa-0000-0000-0000-000000000006',1.0,'s');
             INSERT INTO id_checks VALUES (5,'aaaaaaaa-0000-0000-0000-000000000006','unmatched',NULL,NULL,'t');",
        )
        .unwrap();
        let lib = Library { conn: c };
        let q = lib.review_queue(50).unwrap();
        let u = q.iter().find(|i| i.passage_id == 5).expect("unmatched must be reachable");
        assert_eq!(u.severity, "unverified");
        assert_eq!(u.rank, 5, "and it must sort behind every real finding");
        assert_eq!(q[0].passage_id, 2, "a real contradiction still leads");
    }

    /// A decided passage comes back carrying its judgement, so it can be found
    /// again and withdrawn. It is a separate grade on the page and off by
    /// default, so the working queue still shortens as it is answered.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_decision_is_remembered_and_can_be_withdrawn() {
        let tmp = std::env::temp_dir().join(format!("vaino-rev-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = reviewable();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        let lib = Library::open(&tmp).unwrap();
        assert!(lib.review_queue(50).unwrap()[0].decision.is_none());

        store.record_review(2, "kept", None, None).unwrap();
        let q = lib.review_queue(50).unwrap();
        assert_eq!(q[0].decision.as_deref(), Some("kept"),
                   "a decided card must still be findable");
        assert!(!q[0].applied, "recorded is not applied");
        assert_eq!(lib.review_progress().decided, 1);

        // Undo: the judgement is withdrawn and the passage is a question again.
        store.clear_review(2).unwrap();
        assert!(lib.review_queue(50).unwrap()[0].decision.is_none());
        assert_eq!(lib.review_progress().decided, 0);
        assert!(store.clear_review(2).is_err(), "nothing to withdraw twice");

        store.record_review(2, "kept", None, None).unwrap();

        // Changing one's mind overwrites rather than duplicating.
        store.record_review(2, "reassigned", Some("aaaaaaaa-0000-0000-0000-000000000002"), None).unwrap();
        assert_eq!(lib.review_progress().decided, 1);
        let (d, m): (String, Option<String>) = store
            .conn
            .query_row("SELECT decision, chosen_mbid FROM id_reviews WHERE passage_id = 2",
                       [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(d, "reassigned");
        assert_eq!(m.as_deref(), Some("aaaaaaaa-0000-0000-0000-000000000002"));
        let _ = std::fs::remove_file(&tmp);
    }

    /// A judgement already written into the library cannot be withdrawn by
    /// deleting the record of it: that would leave `passage_recordings`
    /// changed with nothing left saying what it used to be or why. Undo on the
    /// page refuses, and says to use `apply_reviews --revert`, which restores
    /// the old id and clears the row together.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn an_applied_decision_cannot_be_quietly_withdrawn() {
        let tmp = std::env::temp_dir().join(format!("vaino-rev3-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = reviewable();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        let lib = Library::open(&tmp).unwrap();
        let new_id = "aaaaaaaa-0000-0000-0000-000000000002";
        store.record_review(2, "reassigned", Some(new_id), None).unwrap();

        // What the old id was is captured when the decision is made, because
        // applying it overwrites the only other copy.
        let previous: Option<String> = store
            .conn
            .query_row("SELECT previous_mbid FROM id_reviews WHERE passage_id = 2",
                       [], |r| r.get(0))
            .unwrap();
        assert_eq!(previous.as_deref(), Some("aaaaaaaa-0000-0000-0000-000000000001"));

        // `apply_reviews` stamps this once the library has been changed.
        store.conn.execute(
            "UPDATE id_reviews SET applied_at = datetime('now') WHERE passage_id = 2", [])
            .unwrap();

        assert!(store.clear_review(2).is_err(), "an applied decision must not just vanish");
        assert!(store.record_review(2, "kept", None, None).is_err(),
                "nor be silently overwritten by a different answer");
        let q = lib.review_queue(50).unwrap();
        assert!(q[0].applied, "the page has to be able to show it as applied");
    }

    /// An artist correction is independent of the recording decision
    /// `[SPEC-SUI-197]` -- it can be made, found again and withdrawn without
    /// `decision` ever being set, and captures the recording it was made
    /// against (the heavier of passage 2's two links, exactly as
    /// `previous_mbid` above captures the same thing for a reassignment).
    #[cfg(feature = "sampo-support")]
    #[test]
    fn an_artist_correction_is_recorded_and_can_be_withdrawn() {
        let tmp = std::env::temp_dir().join(format!("vaino-art-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = reviewable();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        let lib = Library::open(&tmp).unwrap();
        assert!(lib.review_queue(50).unwrap()[0].artist_review.is_none());

        store.record_artist_review(2, "artist-mbid-1", "The Real Artist").unwrap();
        let q = lib.review_queue(50).unwrap();
        assert_eq!(q[0].artist_review.as_deref(), Some("The Real Artist"));
        assert!(!q[0].artist_review_applied);
        assert_eq!(q[0].decision, None, "an artist correction must not touch `decision`");

        let recording = "aaaaaaaa-0000-0000-0000-000000000001";
        let prev: Option<String> = store
            .conn
            .query_row(
                "SELECT previous_artist_mbid FROM artist_reviews WHERE recording_mbid = ?1",
                [recording], |r| r.get(0))
            .expect("the row must be keyed by recording_mbid, like previous_mbid does");
        assert_eq!(prev, None, "this recording has no existing credit to capture");

        store.clear_artist_review(2).unwrap();
        assert!(lib.review_queue(50).unwrap()[0].artist_review.is_none());
        assert!(store.clear_artist_review(2).is_err(), "nothing to withdraw twice");

        // Committing twice updates the one row, the same `ON CONFLICT` shape
        // every other decision here uses.
        store.record_artist_review(2, "artist-mbid-1", "First Try").unwrap();
        store.record_artist_review(2, "artist-mbid-2", "Better Name").unwrap();
        let n: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM artist_reviews", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "a second commit must update, not add a row");
        assert_eq!(lib.review_queue(50).unwrap()[0].artist_review.as_deref(), Some("Better Name"));

        // Applied is a different, non-withdrawable state, the same as a
        // recording reassignment.
        store.conn.execute(
            "UPDATE artist_reviews SET applied_at = datetime('now') WHERE recording_mbid = ?1",
            [recording])
            .unwrap();
        assert!(lib.review_queue(50).unwrap()[0].artist_review_applied);
        assert!(store.clear_artist_review(2).is_err(),
                "an applied correction must not just vanish");
        assert!(store.record_artist_review(2, "artist-mbid-3", "Yet Another").is_err(),
                "nor be silently overwritten by a different answer");
    }

    /// The bug keying by `passage_id` would have been: two passages sharing
    /// one recording each get their own, silently conflicting correction row.
    /// Keyed by `recording_mbid`, a second passage naming the same recording
    /// finds and updates the SAME row -- an unapplied correction is still a
    /// mutable draft regardless of which card it is edited from -- rather
    /// than starting an independent one nothing would ever reconcile.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn two_passages_sharing_a_recording_share_one_artist_correction() {
        let tmp = std::env::temp_dir().join(format!("vaino-art3-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = reviewable();
            // Passage 3 now ALSO names passage 2's recording -- the "same
            // song, two files" case a correction must not see as two
            // separate questions.
            c.execute(
                "INSERT INTO passage_recordings VALUES \
                    (3,'aaaaaaaa-0000-0000-0000-000000000001',1.0,'s')",
                [],
            )
            .unwrap();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();

        store.record_artist_review(2, "artist-mbid-1", "The Real Artist").unwrap();
        let n: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM artist_reviews", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);

        // Reached from the OTHER passage, the SAME recording's correction is
        // what is found and updated -- an unapplied correction is still a
        // mutable draft regardless of which card it is edited from, the same
        // as changing your mind about a reassignment before it is applied.
        // What it must NOT do is create a second, independent row.
        store.record_artist_review(3, "artist-mbid-2", "A Different Answer").unwrap();
        let n2: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM artist_reviews", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n2, 1, "a second passage's edit must update the shared row, not add one");
        let name: String = store
            .conn
            .query_row(
                "SELECT artist_name FROM artist_reviews WHERE recording_mbid = 'aaaaaaaa-0000-0000-0000-000000000001'",
                [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "A Different Answer");
        let _ = std::fs::remove_file(&tmp);
    }

    /// The recording has to exist before its credit can be corrected -- "the
    /// recording is right" presupposes one was chosen.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn an_artist_correction_needs_a_recording_to_correct() {
        let lib_conn = reviewable();
        // Passage 3 in `reviewable()`'s fixture has a link; an entirely
        // unlinked passage id (none in the fixture) is what this checks.
        let tmp = std::env::temp_dir().join(format!("vaino-art2-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        lib_conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        let store = PlayerStore::open(&tmp).unwrap();
        assert!(store.record_artist_review(99999, "artist-mbid-1", "Nobody").is_err());
    }

    /// The resume-save interval persists like the others `[REQ-VIS-155]`, and
    /// a value from disk is clamped on the way in -- every one of these writes
    /// lands on the appliance's most volatile partition `[PI-C-010]`, so the
    /// setting that governs how many there are must not be settable to zero by
    /// a corrupted row.
    /// **Every setting survives a save and a load** `[SPEC-SC-099]`.
    ///
    /// This is what makes `Settings::KEYS` the single source rather than one of
    /// three lists that have to agree. A field added to the struct and to
    /// `value_of` but forgotten in `set` loses itself on the next restart, and
    /// silently — which is exactly what the old positional `?11` / `r.get(10)`
    /// pairing did whenever a column was added in the wrong place.
    #[test]
    fn every_setting_survives_a_round_trip() {
        // Deliberately unlike the defaults in every field, so a setting that
        // quietly falls back to its default fails here rather than passing by
        // coincidence.
        let want = Settings {
            volume: 0.375,
            skip_fade_ms: 1_234,
            skip_lead_ms: 321,
            resume_save_ms: 7_000,
            skip_suppress_h: 99,
            dequeue_suppress_h: 7,
            queue_depth: 9,
            sample_interval_ms: 1_500,
            cue_sheets: true,
            covers: true,
            lyrics_cache: true,
            lyrics_sidecar: true,
        };

        let mut got = Settings::default();
        for key in Settings::KEYS {
            let value =
                want.value_of(key).unwrap_or_else(|| panic!("{key} has no value to store"));
            got.set(key, &value);
        }

        assert_eq!(got, want, "a setting was written but not read back");
    }

    /// The other direction: a key listed with no field behind it.
    #[test]
    fn the_key_list_covers_the_whole_struct() {
        let s = Settings::default();
        for key in Settings::KEYS {
            assert!(s.value_of(key).is_some(), "{key} is listed but stores nothing");
        }
        assert!(s.value_of("invented").is_none(), "and an unknown key stores nothing");
    }

    #[test]
    fn the_resume_interval_persists_and_is_clamped() {
        let tmp = std::env::temp_dir().join(format!("vaino-rs2-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let store = PlayerStore::open(&tmp).unwrap();
        assert!(store.load_settings().is_none(), "nothing saved yet");

        let want = Settings {
            volume: 0.5,
            skip_fade_ms: 2_000,
            skip_lead_ms: 500,
            resume_save_ms: 30_000,
            skip_suppress_h: 96,
            dequeue_suppress_h: 12,
            queue_depth: 7,
            sample_interval_ms: 3_000,
            cue_sheets: true,
            covers: true,
            lyrics_cache: true,
            lyrics_sidecar: true,
        };
        store.save_settings(&want).unwrap();
        let got = store.load_settings().unwrap();
        assert!((got.volume - want.volume).abs() < 1e-6);
        assert_eq!(
            (got.skip_fade_ms, got.skip_lead_ms, got.resume_save_ms),
            (want.skip_fade_ms, want.skip_lead_ms, want.resume_save_ms)
        );
        assert_eq!(
            (got.skip_suppress_h, got.dequeue_suppress_h, got.queue_depth, got.sample_interval_ms),
            (96, 12, 7, 3_000)
        );
        assert!(got.cue_sheets, "a choice about the music folder must survive a restart");

        // A library written before a column existed reads as THAT field's
        // default, not as zero, and not by failing the whole read.
        let d = Settings::default();
        // A setting that is not stored falls back to **its own** default, not
        // to zero and not by failing the whole read. Under the old columns this
        // was a NULL; now it is simply a key that is not there.
        for (key, expect) in [
            ("resume_save_ms", d.resume_save_ms),
            ("skip_suppress_h", d.skip_suppress_h),
            ("dequeue_suppress_h", d.dequeue_suppress_h),
            ("sample_interval_ms", d.sample_interval_ms),
        ] {
            store
                .conn
                .execute("DELETE FROM player_settings WHERE key = ?1", [key])
                .unwrap();
            let got = store.load_settings().unwrap();
            let actual = match key {
                "resume_save_ms" => got.resume_save_ms,
                "skip_suppress_h" => got.skip_suppress_h,
                "dequeue_suppress_h" => got.dequeue_suppress_h,
                _ => got.sample_interval_ms,
            };
            assert_eq!(actual, expect, "{key} must fall back to its own default");
        }
        store.conn.execute("DELETE FROM player_settings WHERE key = 'queue_depth'", []).unwrap();
        assert_eq!(store.load_settings().unwrap().queue_depth, d.queue_depth);

        // And an unreadable value costs that setting only.
        store
            .conn
            .execute("UPDATE player_settings SET value = 'nonsense' WHERE key = 'skip_fade_ms'", [])
            .unwrap();
        let got = store.load_settings().unwrap();
        assert_eq!(got.skip_fade_ms, d.skip_fade_ms, "unreadable falls back");
        assert_eq!(got.skip_lead_ms, 500, "and the rest still load");

        let _ = std::fs::remove_file(&tmp);
    }

    /// The vocabulary is enforced where it is written, because that is the only
    /// place it can be. A reassignment with nothing to reassign to is the case
    /// a careless request would produce.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_reassignment_must_name_a_recording() {
        let tmp = std::env::temp_dir().join(format!("vaino-rev2-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = reviewable();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        assert!(store.record_review(2, "reassigned", None, None).is_err());
        assert!(store.record_review(2, "reassigned", Some(""), None).is_err());
        assert!(store.record_review(2, "nonsense", None, None).is_err());
        assert!(store.record_review(2, "deferred", None, None).is_ok());
        let _ = std::fs::remove_file(&tmp);
    }

    /// A boundary edit is readable back through `Library`, and committing
    /// twice updates the one row rather than adding a second `[SPEC021 §2]`.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_boundary_edit_is_recorded_and_updates_in_place() {
        let tmp = std::env::temp_dir().join(format!("vaino-bnd-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = fixture();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        store.record_boundary_review(2, 2000, 290_000, 500, 1500, -1.0).unwrap();

        let lib = Library::open(&tmp).unwrap();
        let got = lib.boundary_review(2).expect("a committed edit must be readable back");
        assert_eq!((got.start_ms, got.end_ms), (2000, 290_000));
        assert_eq!(got.lead_in_ms, Some(500));
        assert_eq!(got.lead_out_ms, Some(1500));
        assert!((got.gain_db.unwrap() - -1.0).abs() < 1e-9);

        store.record_boundary_review(2, 2100, 291_000, 400, 1400, -0.5).unwrap();
        let n: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM boundary_reviews", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "a second commit must update, not add a row");
        let got2 = Library::open(&tmp).unwrap().boundary_review(2).unwrap();
        assert_eq!(got2.start_ms, 2100, "the update must actually take");

        let _ = std::fs::remove_file(&tmp);
    }

    /// The pre-edit span is captured for sync `[SPEC-DF-102]`, and stays
    /// pinned to the true original across a second commit -- the same
    /// "read the live table, not our own prior row" trick `previous_mbid`
    /// already relies on.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_boundary_edit_captures_its_pre_edit_span_for_sync() {
        let tmp = std::env::temp_dir().join(format!("vaino-bnd3-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = fixture();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        store.record_boundary_review(2, 2000, 290_000, 500, 1500, -1.0).unwrap();

        let row = |c: &Connection| -> (String, String, i64, i64, Option<i64>, Option<i64>, Option<f64>) {
            c.query_row(
                "SELECT audio_md5, orig_kind, orig_start_ms, orig_end_ms, \
                        orig_lead_in_ms, orig_lead_out_ms, orig_gain_db \
                   FROM boundary_reviews WHERE passage_id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
            )
            .unwrap()
        };
        let (md5, kind, os, oe, oli, olo, og) = row(&store.conn);
        assert_eq!((md5.as_str(), kind.as_str()), ("md5", "radio"));
        assert_eq!((os, oe), (1200, 298_000), "must be the passage's ORIGINAL span, not the draft");
        assert_eq!((oli, olo), (Some(3000), Some(4000)));
        assert!((og.unwrap() - -2.5).abs() < 1e-9);

        // A second commit, before anything applies it, must not move the
        // baseline to the FIRST draft's values -- `passages` itself has not
        // changed, so re-reading it gives the same true original.
        store.record_boundary_review(2, 2100, 291_000, 400, 1400, -0.5).unwrap();
        let (_, _, os2, oe2, ..) = row(&store.conn);
        assert_eq!((os2, oe2), (1200, 298_000), "the baseline must not drift on a re-commit");

        let _ = std::fs::remove_file(&tmp);
    }

    /// Changing a decision already folded into `passages` would corrupt the
    /// baseline `[SPEC-DF-102]` sync depends on, the same reason
    /// `record_review` refuses the equivalent case for a reassignment.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_boundary_edit_cannot_be_recommitted_once_applied() {
        let tmp = std::env::temp_dir().join(format!("vaino-bnd4-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = fixture();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        store.record_boundary_review(2, 2000, 290_000, 500, 1500, -1.0).unwrap();
        store
            .conn
            .execute(
                "UPDATE boundary_reviews SET applied_at = datetime('now') WHERE passage_id = 2",
                [],
            )
            .unwrap();
        assert!(store.record_boundary_review(2, 2200, 292_000, 300, 1300, -0.2).is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    /// `start_ms >= end_ms` is nonsense no caller should be able to write,
    /// validated where it is written because this is the only writer.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_boundary_edit_cannot_invert_start_and_end() {
        let tmp = std::env::temp_dir().join(format!("vaino-bnd2-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = fixture();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        assert!(store.record_boundary_review(2, 5000, 5000, 0, 0, 0.0).is_err());
        assert!(store.record_boundary_review(2, 6000, 5000, 0, 0, 0.0).is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    /// A library nothing has ever edited has no `boundary_reviews` table at
    /// all -- the same "missing table means never looked, not nothing found"
    /// distinction `review_queue` already has to make.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_passage_never_edited_has_no_boundary_review() {
        let lib = Library { conn: fixture() };
        assert!(lib.boundary_review(2).is_none());
    }

    /// A library the pass has never touched has no `id_checks` table at all,
    /// and a query naming a missing table FAILS rather than returning nothing.
    /// That mistake blanked the browse page twice; nothing to review has to be
    /// distinguishable from a broken page.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn a_library_without_findings_reviews_empty_rather_than_failing() {
        let lib = Library { conn: fixture() };
        assert!(lib.review_queue(50).unwrap().is_empty());
        let p = lib.review_progress();
        assert!(!p.ran, "the page must be able to say the pass never ran");
        assert_eq!(p.contradicted, 0);
    }

    #[test]
    fn reads_a_radio_passage_with_its_fades() {
        let lib = Library { conn: fixture() };
        let e = lib.passage(2).unwrap();
        assert_eq!(e.start_ms, 1200);
        assert_eq!(e.end_ms, 298_000);
        assert_eq!(e.lead_in_ms, 3000);
        assert_eq!(e.lead_out_ms, 4000);
        assert!((e.gain_db - -2.5).abs() < 1e-6);
        assert_eq!(e.path, PathBuf::from("/m/a.mp3"));
    }

    #[test]
    fn null_leads_become_zero_not_a_guess() {
        let lib = Library { conn: fixture() };
        let e = lib.passage(1).unwrap(); // album passage, leads NULL
        assert_eq!(e.lead_in_ms, 0);
        assert_eq!(e.lead_out_ms, 0);
        assert_eq!(e.gain_db, 0.0);
    }

    #[test]
    fn selection_is_radio_only() {
        let lib = Library { conn: fixture() };
        assert_eq!(lib.count_radio().unwrap(), 1);
        let picked = lib.random_radio(10).unwrap();
        assert_eq!(picked.len(), 1, "the album passage must not be selectable");
        assert_eq!(picked[0].passage_id, 2);
    }

    /// A medley passage has several recordings; the query must return ONE row
    /// and pick deterministically, or the passage appears twice in every pool.
    #[test]
    fn a_medley_passage_yields_one_row_and_the_heaviest_recording() {
        let lib = Library { conn: fixture() };
        let e = lib.passage(2).unwrap();
        assert_eq!(e.mbid.as_deref(), Some("aaaaaaaa-0000-0000-0000-000000000001"), "highest weight wins");
        assert_eq!(lib.random_radio(10).unwrap().len(), 1, "one row, not two");
    }

    #[test]
    fn an_unidentified_passage_has_no_mbid() {
        let lib = Library { conn: fixture() };
        assert_eq!(lib.passage(1).unwrap().mbid, None);
    }

    /// Play history is what the next selection reads `[REQ-PD-110]`, and it is
    /// keyed by MBID so it survives a rescan that renumbers passages.
    #[test]
    fn a_play_is_recorded_with_its_mbid() {
        let p = std::env::temp_dir().join(format!("vaino_ph_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        let st = PlayerStore::open(&p).unwrap();
        // The table is PlayerStore::open's now.
        st.record_play(7, Some("aaaaaaaa-0000-0000-0000-000000000001"), 90_000, 180_000).unwrap();
        st.record_play(8, None, 90_000, 180_000).unwrap();
        let rows: Vec<(i64, Option<String>)> = st
            .conn
            .prepare("SELECT passage_id, mbid FROM listener_play_history ORDER BY play_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows, vec![(7, Some("aaaaaaaa-0000-0000-0000-000000000001".into())), (8, None)],
                   "an unidentified passage still records a play");
        let _ = std::fs::remove_file(&p);
    }

    /// An existing library, written before any of this, must open and work.
    ///
    /// This is the shipping question rather than a unit one: the appliance's
    /// database has a `player_state` with none of the newer columns and no
    /// `listener_rejections` at all. A query naming a column that does not
    /// exist fails outright rather than returning nothing, so a missed
    /// migration is a player that will not start.
    #[test]
    fn a_library_from_before_all_this_still_opens() {
        let tmp = std::env::temp_dir().join(format!(
            "vaino_legacy_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&tmp);
        // Exactly the old shape: the original five columns, nothing since.
        {
            let c = Connection::open(&tmp).unwrap();
            c.execute_batch(
                "CREATE TABLE player_state (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     passage_id INTEGER, position_ms INTEGER NOT NULL DEFAULT 0,
                     playing INTEGER NOT NULL DEFAULT 0, volume REAL NOT NULL DEFAULT 1.0,
                     updated_at TEXT NOT NULL);
                 INSERT INTO player_state (id, position_ms, playing, volume, updated_at)
                     VALUES (1, 4321, 1, 0.75, datetime('now'));",
            )
            .unwrap();
        }

        let store = PlayerStore::open(&tmp).expect("an old library must still open");

        // The resume point survives the migration -- it is the one thing in
        // that row a listener would notice losing.
        assert_eq!(store.load().unwrap(), Some((None, 4321, true)));

        // New settings read as their defaults, not as zero.
        let s = store.load_settings().expect("settings readable");
        let d = Settings::default();
        assert!((s.volume - 0.75).abs() < 1e-6, "the old value is kept");
        assert_eq!(s.skip_suppress_h, d.skip_suppress_h);
        assert_eq!(s.dequeue_suppress_h, d.dequeue_suppress_h);
        assert_eq!(s.queue_depth, d.queue_depth);
        assert_eq!(s.sample_interval_ms, d.sample_interval_ms);

        // And the table the player writes rejections to now exists.
        store.record_rejection(Rejection::Skip, 1, Some("m"), Some(10_000), Some(180_000)).unwrap();
        assert_eq!(store.last_rejected(Rejection::Skip).unwrap().len(), 1);

        // Writing back does not fail on the migrated row either.
        store.save_settings(&s).unwrap();
        let _ = std::fs::remove_file(&tmp);
    }

    /// A library with `listener_play_history`/`listener_rejections` already in
    /// place -- any build before `[REQ-VIS-250]` -- must gain the two new
    /// columns rather than failing every write with "no such column".
    #[test]
    fn an_existing_history_table_gains_the_played_columns() {
        let tmp = std::env::temp_dir().join(format!(
            "vaino_histmig_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = Connection::open(&tmp).unwrap();
            c.execute_batch(
                "CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                     played_at INTEGER NOT NULL, passage_id INTEGER, mbid TEXT);
                 CREATE TABLE listener_rejections (rejection_id INTEGER PRIMARY KEY,
                     rejected_at INTEGER NOT NULL, kind TEXT NOT NULL,
                     passage_id INTEGER, mbid TEXT);
                 INSERT INTO listener_play_history VALUES (1, 100, 7, 'm');",
            )
            .unwrap();
        }

        let store = PlayerStore::open(&tmp).expect("a pre-migration history table must still open");

        // The old row survives, unharmed and now reading its new columns as
        // absent rather than missing -- the query naming them no longer fails.
        let old: (i64, Option<i64>, Option<i64>) = store
            .conn
            .query_row(
                "SELECT passage_id, heard_ms, span_ms FROM listener_play_history WHERE play_id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(old, (7, None, None), "an old row keeps its data and gains absent new columns");

        // And a fresh write goes in with the new columns filled.
        let id = store.record_play(8, Some("n"), 90_000, 180_000).unwrap();
        store.finish_play(id, 150_000).unwrap();
        let heard: i64 = store
            .conn
            .query_row("SELECT heard_ms FROM listener_play_history WHERE play_id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(heard, 150_000, "finish_play must update the row record_play wrote");
        let _ = std::fs::remove_file(&tmp);
    }

    /// A rejection is written where suppression can see it and the weighting
    /// cannot, and the two kinds stay apart `[SPEC-PLAY-050]`, `[SPEC-PLAY-055]`.
    #[test]
    fn rejections_are_recorded_apart_from_plays_and_by_kind() {
        // Unique per run, not merely per process: a process id is reused, and a
        // leftover file would meet a bare CREATE TABLE below.
        let tmp = std::env::temp_dir().join(format!(
            "vaino_sk_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&tmp);
        // Both tables are PlayerStore::open's now.
        let st = PlayerStore::open(&tmp).unwrap();
        let skipped = "aaaaaaaa-0000-0000-0000-000000000009";
        let dropped = "aaaaaaaa-0000-0000-0000-00000000000d";
        st.record_rejection(Rejection::Skip, 11, Some(skipped), Some(10_000), Some(180_000)).unwrap();
        st.record_rejection(Rejection::Dequeue, 12, Some(dropped), None, None).unwrap();

        // Each kind sees only its own: they earn different windows, so mixing
        // them would apply the wrong one.
        let skips = st.last_rejected(Rejection::Skip).unwrap();
        let deqs = st.last_rejected(Rejection::Dequeue).unwrap();
        assert!(skips.contains_key(skipped) && !skips.contains_key(dropped));
        assert!(deqs.contains_key(dropped) && !deqs.contains_key(skipped));

        // Neither becomes a play.
        let plays: i64 = st
            .conn
            .query_row("SELECT COUNT(*) FROM listener_play_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(plays, 0, "a rejection must never become a play");

        // Only the most recent of a kind matters: a window, not an accumulation.
        st.record_rejection(Rejection::Skip, 11, Some(skipped), Some(15_000), Some(180_000)).unwrap();
        assert_eq!(st.last_rejected(Rejection::Skip).unwrap().len(), 1);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn resume_state_round_trips() {
        let dir = std::env::temp_dir().join(format!("vaino_ps_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let st = PlayerStore::open(&dir).unwrap();
        assert!(st.load().unwrap().is_none(), "first run has no resume point");
        st.save(Some(42), 61_500, true).unwrap();
        assert_eq!(st.load().unwrap(), Some((Some(42), 61_500, true)));
        // saving again must UPDATE, not accumulate rows
        st.save(Some(43), 100, false).unwrap();
        assert_eq!(st.load().unwrap(), Some((Some(43), 100, false)));
        let n: i64 = st.conn.query_row("SELECT COUNT(*) FROM player_state", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "resume state is a single row");
        let _ = std::fs::remove_file(&dir);
    }

    /// The regression `[REQ-VIS-255]` exists for: the first version called
    /// `sync_os_utc_offset` through `Library`'s connection, which is opened
    /// read-only, so the write silently failed on every real run while every
    /// unit test -- built on a writable in-memory `Connection` -- passed.
    /// This goes through `PlayerStore::open` the way production actually
    /// does, so a future version that makes the same mistake fails here too.
    #[test]
    fn sync_utc_offset_actually_reaches_disk() {
        let Some(os) = crate::director::program::os_utc_offset_minutes() else {
            return; // nothing to prove on a platform this cannot ask
        };
        let dir = std::env::temp_dir().join(format!("vaino_tz_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let st = PlayerStore::open(&dir).unwrap();
        // `listener_settings` is Sampo's table, not one `PlayerStore::open`
        // creates -- built here the way a real library already has it.
        let wrong = if os == 0 { 60 } else { 0 };
        st.conn
            .execute_batch(&format!(
                "CREATE TABLE listener_settings (id INTEGER PRIMARY KEY,
                     utc_offset_minutes INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL);
                 INSERT INTO listener_settings (id, utc_offset_minutes, updated_at)
                     VALUES (1, {wrong}, 't');"
            ))
            .unwrap();

        st.sync_utc_offset();

        let after: i64 = st
            .conn
            .query_row("SELECT utc_offset_minutes FROM listener_settings WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(after, os, "the write must reach disk through PlayerStore's own connection");
        let _ = std::fs::remove_file(&dir);
    }

    /// Nothing chosen yet reads as `None`, not as an invented address
    /// `[PI3-AIM-020]`, `[REQ-VIS-260]` -- the reconnect timer's job is to do
    /// nothing in that case, and a stray default here would give it
    /// something to page instead.
    #[test]
    fn no_speaker_address_until_one_is_chosen() {
        let dir = std::env::temp_dir().join(format!("vaino_spk_none_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let st = PlayerStore::open(&dir).unwrap();
        assert_eq!(st.load_speaker_address(), None);
        let _ = std::fs::remove_file(&dir);
    }

    /// The whole point `[PI3-AIM-020]`, `[REQ-VIS-260]`: a chosen speaker
    /// round-trips through the same connection production writes it with,
    /// and choosing a second one replaces the first rather than sitting
    /// beside it -- there is exactly one appliance and one answer to "which
    /// speaker", never a history of them.
    #[test]
    fn a_chosen_speaker_round_trips_and_a_later_choice_replaces_it() {
        let dir = std::env::temp_dir().join(format!("vaino_spk_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&dir);
        let st = PlayerStore::open(&dir).unwrap();

        st.save_speaker_address("AA:BB:CC:DD:EE:FF").unwrap();
        assert_eq!(st.load_speaker_address().as_deref(), Some("AA:BB:CC:DD:EE:FF"));

        st.save_speaker_address("11:22:33:44:55:66").unwrap();
        assert_eq!(st.load_speaker_address().as_deref(), Some("11:22:33:44:55:66"));
        let n: i64 = st
            .conn
            .query_row(
                "SELECT COUNT(*) FROM player_settings WHERE key = 'speaker_address'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "a later choice replaces the row rather than adding to it");
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn a_null_lead_yields_a_gapless_handover() {
        use crate::queue::overlap_ms;
        let lib = Library { conn: fixture() };
        let album = lib.passage(1).unwrap();
        let radio = lib.passage(2).unwrap();
        assert_eq!(overlap_ms(&album, &radio), 0, "unanalysed passage must not crossfade");
    }

    /// The naming tables `play_history` reads, filled with one recording that
    /// has both an artist and a chosen release -- enough to exercise every
    /// column the page shows.
    fn historyable() -> Connection {
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
        c
    }

    /// Plays and skips read back newest first, named from MusicBrainz, and
    /// carrying the percentage heard -- the one thing `browse_tracks` never
    /// has to compute `[REQ-VIS-250]`.
    #[test]
    fn play_history_names_and_orders_its_rows() {
        let c = historyable();
        c.execute(
            "INSERT INTO listener_play_history (played_at, mbid, heard_ms, span_ms) \
             VALUES (100, 'aaaaaaaa-0000-0000-0000-000000000001', 150000, 180000)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO listener_rejections (rejected_at, kind, mbid, heard_ms, span_ms) \
             VALUES (200, 'skip', 'aaaaaaaa-0000-0000-0000-000000000001', 9000, 180000)",
            [],
        )
        .unwrap();
        // A dequeue never sounded and must not appear in a *play* history.
        c.execute(
            "INSERT INTO listener_rejections (rejected_at, kind, mbid) \
             VALUES (300, 'dequeue', 'aaaaaaaa-0000-0000-0000-000000000001')",
            [],
        )
        .unwrap();
        let lib = Library { conn: c };

        assert_eq!(lib.play_history_count().unwrap(), 2, "the dequeue is not counted");

        let rows = lib.play_history(10, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, "skip", "newest first");
        assert_eq!(rows[0].title.as_deref(), Some("A Song"));
        assert_eq!(rows[0].artist.as_deref(), Some("A Band"));
        assert_eq!(rows[0].album.as_deref(), Some("An Album"));
        assert!((rows[0].played_pct.unwrap() - 5.0).abs() < 1e-9);
        assert_eq!(rows[1].kind, "play");
        assert!((rows[1].played_pct.unwrap() - 83.333).abs() < 1e-2);

        // Both rows are the same recording, so both offer the same flag
        // subject `[REQ-VIS-265]` -- one checkbox state per track, not per play.
        assert_eq!(rows[0].flag_kind, Some("recording"));
        assert_eq!(rows[0].flag_id.as_deref(), Some("aaaaaaaa-0000-0000-0000-000000000001"));
        assert_eq!(rows[1].flag_kind, rows[0].flag_kind);
        assert_eq!(rows[1].flag_id, rows[0].flag_id);
        assert!(!rows[0].flagged, "nothing has been flagged yet");

        // Paging: asking past the end returns nothing, not an error.
        assert_eq!(lib.play_history(10, 2).unwrap().len(), 0);
    }

    /// Flagging a recording, or a passage that has none yet, and reading the
    /// state back through `play_history` -- the same round trip the page
    /// itself relies on `[REQ-VIS-265]`. A real file, `PlayerStore` writing
    /// and `Library` reading, the same split the page itself uses.
    #[test]
    fn play_history_reflects_a_flag_by_recording_or_by_passage() {
        let tmp = std::env::temp_dir().join(format!("vaino-flag-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        {
            let c = historyable();
            c.execute(
                "INSERT INTO listener_play_history (played_at, mbid, heard_ms, span_ms) \
                 VALUES (100, 'aaaaaaaa-0000-0000-0000-000000000001', 150000, 180000)",
                [],
            )
            .unwrap();
            // A play with no mbid at all -- unidentified, and the case this
            // feature is often most wanted for -- falls back to the passage.
            c.execute(
                "INSERT INTO listener_play_history (played_at, passage_id, heard_ms, span_ms) \
                 VALUES (150, 42, 150000, 180000)",
                [],
            )
            .unwrap();
            c.execute("VACUUM INTO ?1", [tmp.to_string_lossy()]).unwrap();
        }
        let store = PlayerStore::open(&tmp).unwrap();
        let lib = Library::open(&tmp).unwrap();

        store.set_flag("recording", "aaaaaaaa-0000-0000-0000-000000000001", true).unwrap();
        store.set_flag("passage", "42", true).unwrap();
        let rows = lib.play_history(10, 0).unwrap();
        let by_rec = rows.iter().find(|r| r.flag_kind == Some("recording")).unwrap();
        let by_pas = rows.iter().find(|r| r.flag_kind == Some("passage")).unwrap();
        assert!(by_rec.flagged, "the recording flag must be readable back");
        assert_eq!(by_pas.flag_id.as_deref(), Some("42"));
        assert!(by_pas.flagged, "the passage-keyed flag must be readable back");

        store.set_flag("recording", "aaaaaaaa-0000-0000-0000-000000000001", false).unwrap();
        let rows = lib.play_history(10, 0).unwrap();
        let by_rec = rows.iter().find(|r| r.flag_kind == Some("recording")).unwrap();
        assert!(!by_rec.flagged, "unchecking must actually clear it");
        let by_pas = rows.iter().find(|r| r.flag_kind == Some("passage")).unwrap();
        assert!(by_pas.flagged, "clearing one flag must not touch the other");

        let _ = std::fs::remove_file(&tmp);
    }

    /// The vocabulary is enforced by the table itself, the same as `flavor`'s
    /// own `subject_kind` -- there being only one writer does not mean it
    /// cannot pass the wrong string.
    #[test]
    fn set_flag_rejects_an_unknown_subject_kind() {
        let c = historyable();
        let store = PlayerStore { conn: c };
        assert!(store.set_flag("album", "x", true).is_err());
    }

    /// A row written before `heard_ms`/`span_ms` existed reads as "unknown",
    /// never as "0%" `[GOV-SRC-040]`.
    #[test]
    fn a_played_percentage_absent_before_the_migration_reads_as_unknown() {
        let c = historyable();
        c.execute(
            "INSERT INTO listener_play_history (played_at, mbid) \
             VALUES (100, 'aaaaaaaa-0000-0000-0000-000000000001')",
            [],
        )
        .unwrap();
        let lib = Library { conn: c };
        let rows = lib.play_history(10, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].played_pct, None);
    }
}
