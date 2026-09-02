//! The read-only query surface over `vaino.db` `[REQ-VIS-170]`.
//!
//! [`Library::open`] is `SQLITE_OPEN_READ_ONLY` -- the guard that protects
//! the library from a player bug, narrower than "only Sampo writes" per that
//! method's own doc: [`super::PlayerStore`] is the one writable handle and
//! the only place a table gets created. Everything here returns
//! [`crate::queue::QueueEntry`] or a page-shaped struct of its own -- so this
//! layer adds no vocabulary the rest of the player does not already speak.

use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};

use crate::queue::{Naming, QueueEntry};

use super::{BUSY_WAIT, DbError, TAG_TABLE};

pub struct Library {
    conn: Connection,
}

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

/// The one place the passage/file join is written. Every loader below selects
/// these columns in this order, so `row_to_entry` can stay a single function.
/// Kept as columns and source separately so the Program Director can select
/// these columns *plus its own* and still map the row with [`row_to_entry`].
/// A second hand-written copy of this join would be a second place to get the
/// fade columns wrong.
pub(crate) const COLS: &str = "p.passage_id, f.path, p.start_ms, p.end_ms, f.duration_ms, \
                               p.lead_in_ms, p.lead_out_ms, p.gain_db, \
                               p.fade_in_ms, p.fade_out_ms, p.fade_in_curve, p.fade_out_curve, \
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
        // `NOT NULL DEFAULT 20`/`'exponential'` in the schema `[SPEC-SUI-226]`
        // -- unlike lead, a fade always has a value, never "not yet
        // analysed", so no `Option`/`unwrap_or` dance is needed for the
        // numbers. The curve name still falls back defensively: a plain
        // `TEXT` column can hold anything a hand edit or an older row put
        // there, and a bad name should read as the default, not panic.
        fade_in_ms: row.get::<_, i64>(8)?.max(0) as u64,
        fade_out_ms: row.get::<_, i64>(9)?.max(0) as u64,
        fade_in_curve: crate::fade::Curve::parse(&row.get::<_, String>(10)?)
            .unwrap_or(crate::fade::Curve::Exponential),
        fade_out_curve: crate::fade::Curve::parse(&row.get::<_, String>(11)?)
            .unwrap_or(crate::fade::Curve::Exponential),
        // A scalar subquery rather than a join: a passage may legally hold a
        // medley of several recordings `[SPEC-SC-*]`, and a join would silently
        // return that passage twice. Highest weight wins, mbid breaks ties.
        mbid: row.get::<_, Option<String>>(12)?,
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

/// One passage's own facts, for a person to look at `[REQ-VIS-270]` -- the
/// core span/lead/gain/fade/boundary data and every recording it names, but
/// none of Sampo's own decision history or MusicBrainz release candidates:
/// this lives in Vaino, reachable on an appliance with no network reason to
/// exist, so it shows only what is already sitting in the local database.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PassageProfile {
    pub passage_id: i64,
    pub path: String,
    pub format: Option<String>,
    pub duration_ms: u64,
    pub audio_md5: String,
    pub kind: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub lead_in_ms: Option<i64>,
    pub lead_out_ms: Option<i64>,
    pub gain_db: Option<f64>,
    pub fade_in_ms: i64,
    pub fade_out_ms: i64,
    pub fade_in_curve: String,
    pub fade_out_curve: String,
    pub boundary_src: String,
    /// The file's own tag, read regardless of whether a recording is known
    /// `[SPEC-SC-*]` -- self-published or unidentified audio still has a name.
    pub tag_title: Option<String>,
    pub tag_artist: Option<String>,
    pub tag_album: Option<String>,
    pub recordings: Vec<ProfileRecording>,
    /// The other `kind` of this same recording-in-file span, if this
    /// passage's own file has one `[GDE-BMK-030]`.
    pub sibling: Option<ProfileSibling>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProfileRecording {
    pub mbid: String,
    pub weight: f64,
    pub source: String,
    pub title: Option<String>,
    pub artists: Vec<ProfileArtist>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProfileArtist {
    pub name: String,
    pub weight: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProfileSibling {
    pub passage_id: i64,
    pub kind: String,
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
    /// When this row's own fingerprint check actually ran -- so the page can
    /// say what kind of check this is and how current it is, rather than
    /// leaving both to be inferred `[SPEC-SUI-198]`. A passage identified by
    /// some other means since (a release-tracklist match, a hand pick) does
    /// not retroactively update this: it is the AcoustID pass's own answer,
    /// timestamped, not a live view of the current id.
    pub checked_at: String,
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
pub const SEVERITIES: [(&str, u8, &str); 7] = [
    ("no-mbid", 0, "no MusicBrainz id at all -- a migration placeholder"),
    ("wrong-song", 1, "neither the title nor the performer matches"),
    ("wrong-artist", 2, "same title, different performer"),
    ("wrong-title", 3, "same performer, different title"),
    ("different-id", 4, "the same recording under another MBID"),
    ("unverified", 5, "AcoustID does not know this audio; not evidence"),
    // Not a finding at all -- opened by hand, `[SPEC-SUI-199]`, on a passage
    // the fingerprint queue never flagged (or never checked). Ranked last:
    // if a passage is ALSO a real finding, that grade should win, not this.
    ("on-demand", 6, "opened by hand, not flagged by any automatic check"),
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
    pub fade_in_ms: Option<u64>,
    pub fade_out_ms: Option<u64>,
    pub fade_in_curve: Option<String>,
    pub fade_out_curve: Option<String>,
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
                    a.artist_name, a.applied_at, c.checked_at \
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
                let checked_at: String = r.get(13)?;
                Ok(ReviewItem {
                    passage_id: r.get(0)?,
                    stored_mbid,
                    checked_at,
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

    /// One passage's own review card, whether or not the fingerprint pass
    /// ever flagged it `[SPEC-SUI-199]`.
    ///
    /// `review_queue` above is deliberately narrow -- CONTRADICTED findings
    /// only, so a person is never shown the thousands of passages nothing is
    /// wrong with. That narrowness was also, by accident, the only door: a
    /// passage that is simply unchecked, or whose stored id is merely not
    /// the one a person wants, had no way to reach the search-and-reassign
    /// box at all `[SPEC-SUI-196]` even though nothing about that box
    /// actually depends on being a finding. This is the same card, built
    /// for one named passage regardless of `id_checks`.
    ///
    /// The live recording link (`m.mbid`), not `id_checks.stored_mbid` --
    /// that column is the id *at the time the fingerprint check ran*, which
    /// for a never-checked passage does not exist, and for a since-reassigned
    /// one would be stale. `checked_at` absent means never checked, not
    /// "checked and found nothing to say" -- the two are different states, so
    /// severity becomes its own `on-demand` grade rather than either
    /// `unverified` (which claims AcoustID looked and shrugged) or a
    /// fabricated timestamp.
    #[cfg(feature = "sampo-support")]
    pub fn review_item_for(&self, passage_id: i64) -> Option<ReviewItem> {
        if !self.has_table("id_checks") || !self.has_table("id_reviews")
            || !self.has_table("artist_reviews")
        {
            return None;
        }
        let sql = format!(
            "SELECT m.passage_id, COALESCE(m.mbid, 'local:none'), c.score, c.suggested, \
                    {TITLE_EXPR}, {ARTIST_EXPR}, {ALBUM_EXPR}, \
                    v.decision, v.chosen_mbid, v.chosen_release_mbid, v.applied_at, \
                    a.artist_name, a.applied_at, c.checked_at \
               FROM ({NAMED}) m \
               LEFT JOIN file_tags ft ON ft.file_id = m.file_id \
               LEFT JOIN id_checks c ON c.passage_id = m.passage_id \
               LEFT JOIN id_reviews v ON v.passage_id = m.passage_id \
               LEFT JOIN artist_reviews a ON a.recording_mbid = m.mbid \
              WHERE m.passage_id = ?1"
        );
        self.conn
            .query_row(&sql, [passage_id], |r| {
                let raw: Option<String> = r.get(3)?;
                let suggested: Vec<Suggestion> =
                    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                let title: Option<String> = r.get(4)?;
                let artist: Option<String> = r.get(5)?;
                let stored_mbid: String = r.get(1)?;
                let checked_at: Option<String> = r.get(13)?;
                let (severity, rank) = match &checked_at {
                    Some(_) => grade(&stored_mbid, title.as_deref(), artist.as_deref(), &suggested),
                    None => ("on-demand", 6),
                };
                let applied_at: Option<String> = r.get(10)?;
                let artist_review: Option<String> = r.get(11)?;
                let artist_review_applied_at: Option<String> = r.get(12)?;
                Ok(ReviewItem {
                    passage_id: r.get(0)?,
                    stored_mbid,
                    checked_at: checked_at.unwrap_or_else(|| "never".to_string()),
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
            .ok()
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
                "SELECT start_ms, end_ms, lead_in_ms, lead_out_ms, gain_db,
                        fade_in_ms, fade_out_ms, fade_in_curve, fade_out_curve
                   FROM boundary_reviews WHERE passage_id = ?1",
                [passage_id],
                |r| {
                    Ok(BoundaryReview {
                        start_ms: r.get::<_, i64>(0)? as u64,
                        end_ms: r.get::<_, i64>(1)? as u64,
                        lead_in_ms: r.get::<_, Option<i64>>(2)?.map(|v| v as u64),
                        lead_out_ms: r.get::<_, Option<i64>>(3)?.map(|v| v as u64),
                        gain_db: r.get(4)?,
                        fade_in_ms: r.get::<_, Option<i64>>(5)?.map(|v| v as u64),
                        fade_out_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                        fade_in_curve: r.get(7)?,
                        fade_out_curve: r.get(8)?,
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

    /// Everything one passage is, for a person to look at `[REQ-VIS-270]` --
    /// the appliance-side sibling of Sampo's own profile page, scoped to what
    /// is local, already computed, and needs no network: no decision history,
    /// no MusicBrainz release candidates, nothing this build would have to
    /// reach out for. A read, like `browse_tracks` beside it -- same file,
    /// same cost, no decoder, no allocation that scales with the library.
    pub fn passage_profile(&self, passage_id: i64) -> Result<Option<PassageProfile>, DbError> {
        let row = self.conn.query_row(
            "SELECT p.passage_id, f.path, f.format, f.duration_ms, f.audio_md5, \
                    p.kind, p.start_ms, p.end_ms, p.lead_in_ms, p.lead_out_ms, p.gain_db, \
                    p.fade_in_ms, p.fade_out_ms, p.fade_in_curve, p.fade_out_curve, p.boundary_src, \
                    ft.title, ft.artist, ft.album \
               FROM passages p JOIN files f ON f.file_id = p.file_id \
               LEFT JOIN file_tags ft ON ft.file_id = f.file_id \
              WHERE p.passage_id = ?1",
            [passage_id],
            |r| {
                Ok(PassageProfile {
                    passage_id: r.get(0)?,
                    path: r.get(1)?,
                    format: r.get(2)?,
                    duration_ms: r.get::<_, Option<i64>>(3)?.unwrap_or(0).max(0) as u64,
                    audio_md5: r.get(4)?,
                    kind: r.get(5)?,
                    start_ms: r.get(6)?,
                    end_ms: r.get(7)?,
                    lead_in_ms: r.get(8)?,
                    lead_out_ms: r.get(9)?,
                    gain_db: r.get(10)?,
                    fade_in_ms: r.get(11)?,
                    fade_out_ms: r.get(12)?,
                    fade_in_curve: r.get(13)?,
                    fade_out_curve: r.get(14)?,
                    boundary_src: r.get(15)?,
                    tag_title: r.get(16)?,
                    tag_artist: r.get(17)?,
                    tag_album: r.get(18)?,
                    recordings: Vec::new(),
                    sibling: None,
                })
            },
        );
        let mut profile = match row {
            Ok(p) => p,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(DbError::Query(e.to_string())),
        };

        // Every recording this passage names -- a medley legally has more
        // than one `[SPEC-SC-*]` -- each with its own credited artist(s),
        // heaviest first.
        let mut stmt = self.conn.prepare(
            "SELECT pr.mbid, pr.weight, pr.source, r.title \
               FROM passage_recordings pr LEFT JOIN recordings r ON r.mbid = pr.mbid \
              WHERE pr.passage_id = ?1 ORDER BY pr.weight DESC, pr.mbid",
        ).map_err(|e| DbError::Query(e.to_string()))?;
        let mut recordings = stmt
            .query_map([passage_id], |r| {
                Ok(ProfileRecording {
                    mbid: r.get(0)?,
                    weight: r.get(1)?,
                    source: r.get(2)?,
                    title: r.get(3)?,
                    artists: Vec::new(),
                })
            })
            .map_err(|e| DbError::Query(e.to_string()))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| DbError::Query(e.to_string()))?;
        for rec in &mut recordings {
            let mut astmt = self.conn.prepare(
                "SELECT a.name, ra.weight FROM recording_artists ra \
                   JOIN artists a ON a.mbid = ra.artist_mbid \
                  WHERE ra.mbid = ?1 ORDER BY ra.weight DESC, a.name",
            ).map_err(|e| DbError::Query(e.to_string()))?;
            rec.artists = astmt
                .query_map([&rec.mbid], |r| {
                    Ok(ProfileArtist { name: r.get(0)?, weight: r.get(1)? })
                })
                .map_err(|e| DbError::Query(e.to_string()))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|e| DbError::Query(e.to_string()))?;
        }
        profile.recordings = recordings;

        // The other kind of this same recording-in-file span, if there is
        // one `[GDE-BMK-030]` -- exact span match first, true for every
        // passage `tools/ingest_folder.py`/`tools/segment_dao.py --commit`/
        // `tools/backfill_album_cuts.py` ever wrote, all of which give both
        // kinds the identical `start_ms`/`end_ms` `[SPEC-SA-110]`. Falling
        // back to "some other passage on this file naming the same heaviest
        // recording" covers the migrated `inherited:mulib` data instead,
        // whose own radio/album pair legitimately differ in `start_ms` by
        // design -- its radio cut is independently trimmed at the row level.
        let file_id: i64 = self.conn.query_row(
            "SELECT file_id FROM passages WHERE passage_id = ?1", [passage_id], |r| r.get(0))
            .map_err(|e| DbError::Query(e.to_string()))?;
        let mut sibling: Option<(i64, String)> = self.conn.query_row(
            "SELECT passage_id, kind FROM passages \
              WHERE file_id = ?1 AND kind != ?2 AND start_ms = ?3 AND end_ms = ?4",
            rusqlite::params![file_id, profile.kind, profile.start_ms, profile.end_ms],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).ok();
        if sibling.is_none() {
            if let Some(top) = profile.recordings.first() {
                sibling = self.conn.query_row(
                    "SELECT p2.passage_id, p2.kind FROM passages p2 \
                       JOIN passage_recordings pr2 ON pr2.passage_id = p2.passage_id \
                      WHERE p2.file_id = ?1 AND p2.kind != ?2 AND pr2.mbid = ?3 \
                      LIMIT 1",
                    rusqlite::params![file_id, profile.kind, top.mbid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                ).ok();
            }
        }
        profile.sibling = sibling.map(|(passage_id, kind)| ProfileSibling { passage_id, kind });

        Ok(Some(profile))
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
    use crate::db::test_support::*;
    use crate::db::PlayerStore; // the one cross-cutting round-trip test below
    use super::super::ART_TABLE;

    /// Confirmed ids never reach a person: 6,591 of them here, and there is
    /// nothing to decide about a passage the audio agrees with. What does
    /// reach the page arrives with the candidates that dispute it.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn settled_ids_stay_out_of_the_queue() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (3,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
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

    /// The case `review_queue` cannot reach at all: a passage nobody has ever
    /// fingerprinted, opened by a direct link rather than found in the queue
    /// `[SPEC-SUI-199]`. `review_item_for` must still build a real card for
    /// it -- the search box does not need a fingerprint opinion to work.
    #[cfg(feature = "sampo-support")]
    #[test]
    fn on_demand_reaches_a_passage_the_queue_never_flagged() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO passages VALUES (9,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
             INSERT INTO recordings VALUES ('aaaaaaaa-0000-0000-0000-00000000000a','On Demand',NULL,'s');
             INSERT INTO passage_recordings VALUES (9,'aaaaaaaa-0000-0000-0000-00000000000a',1.0,'s');",
        )
        .unwrap();
        let lib = Library { conn: c };

        // Absent entirely from the queue -- no `id_checks` row exists for it.
        let q = lib.review_queue(50).unwrap();
        assert!(q.iter().all(|i| i.passage_id != 9), "not a queue finding");

        let item = lib.review_item_for(9).expect("a card for a named passage");
        assert_eq!(item.passage_id, 9);
        assert_eq!(item.stored_mbid, "aaaaaaaa-0000-0000-0000-00000000000a",
                   "the live recording link, not a stale id_checks snapshot");
        assert_eq!(item.checked_at, "never", "distinct from a real, dated check");
        assert_eq!(item.severity, "on-demand");
        assert!(item.suggested.is_empty(), "AcoustID has no opinion to offer here");

        assert!(lib.review_item_for(999_999).is_none(), "no such passage, no card");
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
            "INSERT INTO passages VALUES (4,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
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
        // And the grades stay in step with the table the page reads. `grade`
        // itself only ever returns 0..=5 -- `on-demand` at rank 6 is assigned
        // directly by `review_item_for`, never by this function, for a
        // passage the fingerprint queue never flagged at all `[SPEC-SUI-199]`.
        for (name, rank, _) in SEVERITIES {
            assert!(rank < 6 || name == "on-demand", "{name} has no place in the order");
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
            "INSERT INTO passages VALUES (6,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
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
            "INSERT INTO passages VALUES (7,1,'radio',0,1000,NULL,NULL,NULL,'ingest:whole-file',20,20,'exponential','exponential');
             INSERT INTO recordings VALUES ('local:audio:abc','My Own Song',NULL,'local:ingest');
             INSERT INTO passage_recordings VALUES (7,'local:audio:abc',1.0,'local:ingest');
             INSERT INTO id_checks VALUES (7,'local:audio:abc','unmatched',NULL,NULL,'t');
             -- and a migration placeholder, which must still be asked about
             INSERT INTO passages VALUES (8,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
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

        // But a locally-ingested recording AcoustID *can* name is the most
        // useful row there is: a placeholder with the real recording beside it.
        lib.conn.execute_batch(
            "INSERT INTO passages VALUES (9,1,'radio',0,1000,NULL,NULL,NULL,'ingest:whole-file',20,20,'exponential','exponential');
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
            "INSERT INTO passages VALUES (5,1,'radio',0,1000,NULL,NULL,NULL,'src',20,20,'exponential','exponential');
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

    #[test]
    fn a_null_lead_yields_a_gapless_handover() {
        use crate::queue::overlap_ms;
        let lib = Library { conn: fixture() };
        let album = lib.passage(1).unwrap();
        let radio = lib.passage(2).unwrap();
        assert_eq!(overlap_ms(&album, &radio), 0, "unanalysed passage must not crossfade");
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
        // subject `[REQ-VIS-265]` -- one checkbox state per recording, not per play.
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

    /// `passage_profile()` [REQ-VIS-270]: core span/lead/gain/fade facts, a
    /// medley's own recordings heaviest-first with their credited artists,
    /// and the file's own tag as a fallback name.
    #[test]
    fn passage_profile_carries_span_recordings_and_tags() {
        let c = reviewable();
        c.execute_batch(
            "INSERT INTO file_tags (file_id, title, artist, album, has_art, scanned_at) \
                 VALUES (1, 'Fallback Title', 'Fallback Artist', 'Fallback Album', 0, 0);
             INSERT INTO artists VALUES
                 ('bbbbbbbb-0000-0000-0000-000000000001', 'A Band', NULL, 's');
             INSERT INTO recording_artists VALUES
                 ('aaaaaaaa-0000-0000-0000-000000000001', 'bbbbbbbb-0000-0000-0000-000000000001',
                  1.0, 's');",
        )
        .unwrap();
        let lib = Library { conn: c };

        let p = lib.passage_profile(2).unwrap().expect("passage 2 exists");
        assert_eq!(p.passage_id, 2);
        assert_eq!(p.kind, "radio");
        assert_eq!((p.start_ms, p.end_ms), (1200, 298000));
        assert_eq!((p.lead_in_ms, p.lead_out_ms), (Some(3000), Some(4000)));
        assert_eq!(p.gain_db, Some(-2.5));
        assert_eq!((p.fade_in_ms, p.fade_out_ms), (20, 20));
        assert_eq!(p.tag_title.as_deref(), Some("Fallback Title"));

        // The medley's heavier recording first `[SPEC-SC-*]`.
        assert_eq!(p.recordings.len(), 2);
        assert_eq!(p.recordings[0].mbid, "aaaaaaaa-0000-0000-0000-000000000001");
        assert_eq!(p.recordings[0].title.as_deref(), Some("Wrong Song"));
        assert_eq!(p.recordings[0].artists.len(), 1);
        assert_eq!(p.recordings[0].artists[0].name, "A Band");
        assert_eq!(p.recordings[1].mbid, "aaaaaaaa-0000-0000-0000-00000000000f");
        assert!(p.recordings[1].title.is_none(), "an unregistered recording has no title row");

        assert!(lib.passage_profile(999).unwrap().is_none(), "a nonexistent passage is None, not an error");
    }

    /// A passage's `album`/`radio` sibling, resolved two ways `[GDE-BMK-030]`:
    /// exact span match (every newer ingest path `[SPEC-SA-110]`), falling
    /// back to "shares this passage's own heaviest recording" for the
    /// migrated `inherited:mulib` data, whose pair legitimately differs in
    /// `start_ms` by design.
    #[test]
    fn passage_profile_finds_its_sibling_by_span_then_by_recording() {
        let c = reviewable();
        c.execute_batch(
            // File 1's passage 1 ('album', 0-300000) shares no span with
            // passage 2 ('radio', 1200-298000) -- exactly the inherited:mulib
            // shape -- but the same recording links them.
            "INSERT INTO passage_recordings VALUES
                 (1, 'aaaaaaaa-0000-0000-0000-000000000001', 1.0, 's');
             -- A second file, whose two passages DO share an exact span --
             -- every passage this library's own newer ingest paths write.
             INSERT INTO files VALUES (2,'md5b','/m/b.mp3',1,1.0,'mp3',200000,'t','t');
             INSERT INTO passages VALUES (10,2,'radio',0,200000,0,0,0.0,'ingest:whole-file',
                                          20,20,'exponential','exponential');
             INSERT INTO passages VALUES (11,2,'album',0,200000,0,0,0.0,'ingest:whole-file',
                                          20,20,'exponential','exponential');",
        )
        .unwrap();
        let lib = Library { conn: c };

        let by_recording = lib.passage_profile(2).unwrap().unwrap();
        let sib = by_recording.sibling.expect("passage 2 must find passage 1 via their shared recording");
        assert_eq!((sib.passage_id, sib.kind.as_str()), (1, "album"));

        let by_span = lib.passage_profile(10).unwrap().unwrap();
        let sib2 = by_span.sibling.expect("passage 10 must find passage 11 by exact span");
        assert_eq!((sib2.passage_id, sib2.kind.as_str()), (11, "album"));
    }
}
