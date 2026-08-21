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
pub(crate) const REVIEW_COLUMNS: [&str; 3] = [
    "chosen_release_mbid TEXT",
    "previous_mbid TEXT",
    "applied_at TEXT",
];

/// Create `id_reviews` and bring it up to date, in one place.
///
/// The table and the columns added to it later are two halves of one schema,
/// and anything that builds one without the other gets a table the queries
/// cannot read. That is not hypothetical: the test fixture did exactly that
/// and every review test failed on `no such column`.
pub(crate) fn ensure_review_table(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(REVIEW_TABLE).map_err(|e| DbError::Open(e.to_string()))?;
    for column in REVIEW_COLUMNS {
        // Already-present columns fail here, which is the expected path on
        // every start after the first.
        let _ = conn.execute(&format!("ALTER TABLE id_reviews ADD COLUMN {column}"), []);
    }
    Ok(())
}

pub(crate) const COLS: &str = "p.passage_id, f.path, p.start_ms, p.end_ms, \
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
        // NULL lead means "not analysed": treat as no fade rather than
        // inventing one. overlap_ms then yields zero and the handover is
        // gapless, which is the safe default [XFD-OV-010].
        lead_in_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0).max(0) as u64,
        lead_out_ms: row.get::<_, Option<i64>>(5)?.unwrap_or(0).max(0) as u64,
        gain_db: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0) as f32,
        // A scalar subquery rather than a join: a passage may legally hold a
        // medley of several recordings `[SPEC-SC-*]`, and a join would silently
        // return that passage twice. Highest weight wins, mbid breaks ties.
        mbid: row.get::<_, Option<String>>(7)?,
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
        ensure_review_table(&conn)?;
        // Columns Sampo fills and the browse queries read `[SPEC-SA-030]`.
        // Created HERE, on every start, rather than in `ensure_tag_table`:
        // that only runs behind the background scan, so a library whose scan
        // was already complete never reached it and browsing died on a missing
        // column. A query naming a column that does not exist fails outright;
        // it does not return nothing.
        for column in ["skip_fade_ms INTEGER", "skip_lead_ms INTEGER",
                       "resume_save_ms INTEGER", "skip_suppress_h INTEGER"] {
            let _ = conn.execute(
                &format!("ALTER TABLE player_state ADD COLUMN {column}"), []);
        }
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
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_release_recordings_mbid \
               ON release_recordings(mbid)", []);
        Ok(Self { conn })
    }

    /// Record a judgement about a questionable id `[REQ-LIB-165]`.
    ///
    /// The decision is validated here rather than trusted from the request:
    /// this is the only writer, so it is the only place the vocabulary can be
    /// enforced. A reassignment must name a recording; a keep must not.
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
    pub fn save_settings(&self, volume: f32, skip_fade_ms: u64, skip_lead_ms: u64,
                         resume_save_ms: u64, skip_suppress_h: u64) -> Result<(), DbError>
    {
        self.conn
            .execute(
                "INSERT INTO player_state
                     (id, volume, skip_fade_ms, skip_lead_ms, resume_save_ms,
                      skip_suppress_h, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5, datetime('now'))
                 ON CONFLICT(id) DO UPDATE SET
                     volume = excluded.volume,
                     skip_fade_ms = excluded.skip_fade_ms,
                     skip_lead_ms = excluded.skip_lead_ms,
                     resume_save_ms = excluded.resume_save_ms,
                     skip_suppress_h = excluded.skip_suppress_h,
                     updated_at = excluded.updated_at",
                rusqlite::params![volume as f64, skip_fade_ms as i64, skip_lead_ms as i64,
                                  resume_save_ms as i64, skip_suppress_h as i64],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Volume, skip fade and skip lead as they were left.
    ///
    /// Absent columns and absent rows both mean "never saved", which is a
    /// first run and not a fault: the caller keeps its defaults.
    pub fn load_settings(&self) -> Option<(f32, u64, u64, u64, u64)> {
        self.conn
            .query_row(
                "SELECT volume, skip_fade_ms, skip_lead_ms, resume_save_ms, skip_suppress_h \n                 FROM player_state WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<f64>>(0)?.unwrap_or(1.0) as f32,
                        r.get::<_, Option<i64>>(1)?.unwrap_or(crate::SKIP_FADE_MS as i64) as u64,
                        r.get::<_, Option<i64>>(2)?.unwrap_or(crate::SKIP_LEAD_MS as i64) as u64,
                        r.get::<_, Option<i64>>(3)?.unwrap_or(crate::RESUME_SAVE_MS as i64) as u64,
                        r.get::<_, Option<i64>>(4)?
                            .unwrap_or(crate::SKIP_SUPPRESS_H as i64) as u64,
                    ))
                },
            )
            .ok()
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
    pub fn record_play(&self, passage_id: i64, mbid: Option<&str>) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO listener_play_history (played_at, passage_id, mbid) \
                 VALUES (strftime('%s','now'), ?1, ?2)",
                rusqlite::params![passage_id, mbid],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Record a skip, for suppression and nothing else `[SPEC-PLAY-050]`.
    ///
    /// A passage the listener rejected before it reached `[SPEC-PLAY-010]`'s
    /// threshold did not play, so it must not enter `listener_play_history` —
    /// but offering it back an hour later is its own kind of wrong. This is the
    /// narrowest record that fixes that: a timestamp per recording, read only
    /// by the eligibility gate, feeding no ramp, no artist damping and no count.
    pub fn record_skip(&self, passage_id: i64, mbid: Option<&str>) -> Result<(), DbError> {
        self.conn
            .execute(
                "INSERT INTO listener_skip_history (skipped_at, passage_id, mbid) \
                 VALUES (strftime('%s','now'), ?1, ?2)",
                rusqlite::params![passage_id, mbid],
            )
            .map(|_| ())
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// When each recording was last skipped. Only the most recent matters:
    /// suppression is a window, not an accumulation.
    pub fn last_skipped(&self) -> Result<std::collections::HashMap<String, i64>, DbError> {
        let mut q = self
            .conn
            .prepare(
                "SELECT mbid, MAX(skipped_at) FROM listener_skip_history \
                 WHERE mbid IS NOT NULL GROUP BY mbid",
            )
            .map_err(|e| DbError::Query(e.to_string()))?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
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

/// One candidate identity for a passage, as AcoustID reports it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Suggestion {
    pub mbid: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub score: f64,
}

/// A passage whose audio does not match the id it carries `[REQ-LIB-165]`.
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
}

/// A release the chosen recording appears on, for naming the album
/// `[REQ-LIB-165]`.
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
        if !self.has_table("id_checks") || !self.has_table("id_reviews") {
            return Ok(Vec::new());
        }
        // Decided passages come back too, carrying their judgement, so that a
        // decision can be found again and withdrawn. They are a separate grade
        // on the page and switched off by default, so working through the
        // queue still shortens it.
        let sql = format!(
            "SELECT c.passage_id, c.stored_mbid, c.score, c.suggested, \
                    {TITLE_EXPR}, {ARTIST_EXPR}, {ALBUM_EXPR}, \
                    v.decision, v.chosen_mbid, v.chosen_release_mbid, v.applied_at \
               FROM id_checks c \
               JOIN ({NAMED}) m ON m.passage_id = c.passage_id \
               LEFT JOIN file_tags ft ON ft.file_id = m.file_id \
               LEFT JOIN id_reviews v ON v.passage_id = c.passage_id \
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

    /// The stored cover for a passage's chosen release `[REQ-VIS-170]`.
    ///
    /// Looked up through the release Sampo chose, so a folder holding two
    /// albums gives each its own cover. Absent table, absent row and a blob
    /// too small to be a picture all mean the same thing to the caller: no
    /// art, show nothing.
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
                      WHERE pr.passage_id = ?1 AND rr.chosen = 1 \
                        AND a.{col} IS NOT NULL LIMIT 1"
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
             CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY, mbid TEXT);
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
        ensure_review_table(&c).unwrap();
        c
    }

    /// Confirmed ids never reach a person: 6,591 of them here, and there is
    /// nothing to decide about a passage the audio agrees with. What does
    /// reach the page arrives with the candidates that dispute it.
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
        png.extend(std::iter::repeat(0u8).take(600));
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

    /// The resume-save interval persists like the others `[REQ-VIS-155]`, and
    /// a value from disk is clamped on the way in -- every one of these writes
    /// lands on the appliance's most volatile partition `[PI-C-010]`, so the
    /// setting that governs how many there are must not be settable to zero by
    /// a corrupted row.
    #[test]
    fn the_resume_interval_persists_and_is_clamped() {
        let tmp = std::env::temp_dir().join(format!("vaino-rs2-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let store = PlayerStore::open(&tmp).unwrap();
        assert!(store.load_settings().is_none(), "nothing saved yet");

        store.save_settings(0.5, 2_000, 500, 30_000, 96).unwrap();
        let (v, fade, lead, resume, suppress) = store.load_settings().unwrap();
        assert!((v - 0.5).abs() < 1e-6);
        assert_eq!((fade, lead, resume, suppress), (2_000, 500, 30_000, 96));

        // A library written before this column existed reads as the default,
        // not as zero -- which would be a write every tick.
        store.conn.execute("UPDATE player_state SET resume_save_ms = NULL", []).unwrap();
        assert_eq!(store.load_settings().unwrap().3, crate::RESUME_SAVE_MS);
        store.conn.execute("UPDATE player_state SET skip_suppress_h = NULL", []).unwrap();
        assert_eq!(store.load_settings().unwrap().4, crate::SKIP_SUPPRESS_H);
        let _ = std::fs::remove_file(&tmp);
    }

    /// The vocabulary is enforced where it is written, because that is the only
    /// place it can be. A reassignment with nothing to reassign to is the case
    /// a careless request would produce.
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

    /// A library the pass has never touched has no `id_checks` table at all,
    /// and a query naming a missing table FAILS rather than returning nothing.
    /// That mistake blanked the browse page twice; nothing to review has to be
    /// distinguishable from a broken page.
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
        st.conn
            .execute_batch(
                "CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                     played_at INTEGER NOT NULL, passage_id INTEGER, mbid TEXT);",
            )
            .unwrap();
        st.record_play(7, Some("aaaaaaaa-0000-0000-0000-000000000001")).unwrap();
        st.record_play(8, None).unwrap();
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

    /// A skip is written where suppression can see it and the weighting cannot
    /// `[SPEC-PLAY-050]`.
    #[test]
    fn skips_are_recorded_apart_from_plays() {
        let tmp = std::env::temp_dir().join(format!("vaino_sk_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let st = PlayerStore::open(&tmp).unwrap();
        st.conn
            .execute_batch(
                "CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                     played_at INTEGER NOT NULL, passage_id INTEGER, mbid TEXT);
                 CREATE TABLE listener_skip_history (skip_id INTEGER PRIMARY KEY,
                     skipped_at INTEGER NOT NULL, passage_id INTEGER, mbid TEXT);",
            )
            .unwrap();
        let m = "aaaaaaaa-0000-0000-0000-000000000009";
        st.record_skip(11, Some(m)).unwrap();

        // Visible to suppression...
        assert!(st.last_skipped().unwrap().contains_key(m));
        // ...and absent from the table rotation and recovery read.
        let plays: i64 = st
            .conn
            .query_row("SELECT COUNT(*) FROM listener_play_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(plays, 0, "a skip must never become a play");

        // Only the most recent skip matters: a window, not an accumulation.
        st.record_skip(11, Some(m)).unwrap();
        assert_eq!(st.last_skipped().unwrap().len(), 1);
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

    #[test]
    fn a_null_lead_yields_a_gapless_handover() {
        use crate::queue::overlap_ms;
        let lib = Library { conn: fixture() };
        let album = lib.passage(1).unwrap();
        let radio = lib.passage(2).unwrap();
        assert_eq!(overlap_ms(&album, &radio), 0, "unanalysed passage must not crossfade");
    }
}
