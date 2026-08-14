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

pub(crate) const COLS: &str = "p.passage_id, f.path, p.start_ms, p.end_ms, \
                               p.lead_in_ms, p.lead_out_ms, p.gain_db, \
                               (SELECT pr.mbid FROM passage_recordings pr \
                                WHERE pr.passage_id = p.passage_id \
                                ORDER BY pr.weight DESC, pr.mbid LIMIT 1)";
pub(crate) const FROM: &str = "FROM passages p JOIN files f USING (file_id)";

pub(crate) fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueEntry> {
    Ok(QueueEntry {
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
        Ok(Self { conn })
    }

    /// Save the resume point `[REQ-AUD-140]`.
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

    pub fn browse_tracks(&self, f: &BrowseFilter) -> Result<Vec<BrowseTrack>, DbError> {
        // An album is a running order, not an index: opened as one, its tracks
        // belong in the order they were put on the record `[REQ-VIS-190]`.
        // Anywhere else alphabetical is what makes a long list findable.
        // Unnumbered tracks sort after the numbered ones rather than ahead of
        // them, which is where a bare NULL would put them.
        let order = if f.album.is_some() {
            "ORDER BY COALESCE(disc_no, 1), \
                      CASE WHEN track_no IS NULL THEN 1 ELSE 0 END, track_no, \
                      title COLLATE NOCASE"
        } else {
            "ORDER BY title COLLATE NOCASE"
        };
        let sql = format!(
            "SELECT passage_id, title, artist, album, plays, track_no, disc_no FROM ( \
               SELECT m.passage_id, {TITLE_EXPR} AS title, {ARTIST_EXPR} AS artist, \
                      {ALBUM_EXPR} AS album, {PLAYS_EXPR} AS plays, \
                      ft.track_no AS track_no, ft.disc_no AS disc_no \
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
             INSERT INTO passage_recordings VALUES (2,'rec-light',0.3,'s'),
                                                   (2,'rec-main',0.9,'s');",
        )
        .unwrap();
        c
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
        assert_eq!(e.mbid.as_deref(), Some("rec-main"), "highest weight wins");
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
        st.record_play(7, Some("rec-main")).unwrap();
        st.record_play(8, None).unwrap();
        let rows: Vec<(i64, Option<String>)> = st
            .conn
            .prepare("SELECT passage_id, mbid FROM listener_play_history ORDER BY play_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(rows, vec![(7, Some("rec-main".into())), (8, None)],
                   "an unidentified passage still records a play");
        let _ = std::fs::remove_file(&p);
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
