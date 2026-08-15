//! Keeping the part of the library that cannot be rebuilt `[REQ-LIB-160]`.
//!
//! The library file holds two kinds of thing, and they have opposite recovery
//! stories. The **library** — files, passages, recordings, flavor — is derived
//! from the audio on disk, and Sampo can grind it out again from nothing but
//! time. The **listening** — 37,206 plays, 3,261 preferences, the programmes
//! and their seeds — is derived from years of a person using the thing, and
//! nothing anywhere can reproduce it. Lose it and the Program Director is a
//! random shuffle with opinions it can no longer justify.
//!
//! So this copies only the second kind. That choice is what makes the backup
//! small enough to take often: a few megabytes against a library of hundreds,
//! which is the difference between a snapshot every hour and one nobody runs.
//!
//! **A copy, not a dump.** The output is a real SQLite file, openable and
//! queryable, restorable by attaching it. A schema-and-INSERTs text dump would
//! need a working player to be useful, and the moment a backup matters is the
//! moment there isn't one.
//!
//! **Rotating, not overwriting.** Corruption that goes unnoticed for a day
//! would otherwise be faithfully copied over the last good snapshot. Keeping
//! several generations means the damage has to outrun the whole set.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::DbError;

/// Everything a listener created, and nothing a machine can recreate.
///
/// `player_state` is included though it is only the resume point: it is one row
/// and it is the difference between resuming where you were and starting over.
///
/// Deliberately NOT here: `files`, `passages`, `recordings`, `artists`,
/// `releases`, `flavor`, `file_tags`, the caches. All of it is derived, all of
/// it is large, and all of it Sampo can produce again.
pub const LISTENER_TABLES: &[&str] = &[
    "listener_play_history",
    "listener_preferences",
    "listener_likes",
    "listener_programs",
    "listener_program_seeds",
    "listener_occasions",
    "listener_occasion_points",
    "listener_settings",
    "player_state",
];

/// How many snapshots to keep before the oldest is dropped.
///
/// Seven, taken hourly, is most of a day of history — long enough that damage
/// has to go unnoticed for a shift before it reaches every copy, short enough
/// that the set stays small beside the library it sits next to.
pub const KEEP: usize = 7;

/// Where snapshots live: beside the library, in their own directory, so a
/// glob for `*.db` in the data directory cannot sweep them up as libraries.
pub fn dir_for(db: &Path) -> PathBuf {
    db.parent().unwrap_or_else(|| Path::new(".")).join("listener-backups")
}

/// Take one snapshot. Returns where it was written.
///
/// Read-only on the source, so it can run while the player is playing: SQLite
/// in WAL mode lets a reader work through a writer without either waiting.
pub fn snapshot(db: &Path) -> Result<PathBuf, DbError> {
    let dir = dir_for(db);
    std::fs::create_dir_all(&dir)
        .map_err(|e| DbError::Open(format!("create {}: {e}", dir.display())))?;

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out = dir.join(format!("listener-{stamp}.db"));
    // A half-written snapshot must never look like a good one, so it is built
    // under a temporary name and renamed only once it is complete. Rename is
    // atomic; a copy interrupted half way leaves a `.part` nobody will trust.
    let part = dir.join(format!("listener-{stamp}.part"));
    let _ = std::fs::remove_file(&part);

    // The connection owns the SNAPSHOT and attaches the library, not the other
    // way round. Two reasons, and the second is the one that matters: ATTACH
    // cannot create a database from a read-only connection, and this way the
    // library is attached `mode=ro`, so a mistake in the copy below cannot
    // write to the thing being protected.
    let conn = Connection::open(&part)
        .map_err(|e| DbError::Open(format!("create {}: {e}", part.display())))?;
    conn.busy_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| DbError::Open(e.to_string()))?;
    let src = format!(
        "file:{}?mode=ro",
        db.to_string_lossy().replace('?', "%3f").replace('#', "%23")
    );
    conn.execute("ATTACH DATABASE ?1 AS src", [src.as_str()])
        .map_err(|e| DbError::Query(format!("attach {}: {e}", db.display())))?;

    let mut copied = 0usize;
    for table in LISTENER_TABLES {
        // A table the schema has not grown yet is not an error: a fresh
        // library has no likes, and a backup of nothing is still a backup.
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM src.sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            continue;
        }
        conn.execute_batch(&format!(
            "CREATE TABLE main.\"{table}\" AS SELECT * FROM src.\"{table}\";"
        ))
        .map_err(|e| DbError::Query(format!("copy {table}: {e}")))?;
        copied += 1;
    }
    conn.execute_batch("DETACH DATABASE src")
        .map_err(|e| DbError::Query(e.to_string()))?;
    drop(conn);

    if copied == 0 {
        let _ = std::fs::remove_file(&part);
        return Err(DbError::Query("no listener tables to back up".into()));
    }
    std::fs::rename(&part, &out)
        .map_err(|e| DbError::Open(format!("finish {}: {e}", out.display())))?;
    prune(&dir);
    Ok(out)
}

/// Drop the oldest snapshots beyond [`KEEP`].
///
/// Failure here is deliberately silent: an undeletable old snapshot is untidy,
/// and refusing to take new ones over it would turn tidiness into data loss.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut snaps: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("listener-") && n.ends_with(".db"))
        })
        .collect();
    if snaps.len() <= KEEP {
        return;
    }
    // The name carries the timestamp, so sorting by name sorts by age.
    snaps.sort();
    for old in &snaps[..snaps.len() - KEEP] {
        let _ = std::fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(dir: &Path) -> PathBuf {
        let db = dir.join("lib.db");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                 played_at INTEGER, passage_id INTEGER, mbid TEXT);
             CREATE TABLE listener_settings (id INTEGER PRIMARY KEY, artist_time_scale REAL);
             CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT);
             INSERT INTO listener_play_history VALUES (1, 100, 7, 'abc');
             INSERT INTO listener_play_history VALUES (2, 200, 8, 'def');
             INSERT INTO listener_settings VALUES (1, 1.0);
             INSERT INTO files VALUES (1, '/music/a.mp3');",
        )
        .unwrap();
        db
    }

    #[test]
    fn a_snapshot_holds_the_listening_and_not_the_library() {
        let tmp = std::env::temp_dir().join(format!("vaino-bk-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = library(&tmp);

        let out = snapshot(&db).expect("snapshot");
        let c = Connection::open(&out).unwrap();
        let plays: i64 = c
            .query_row("SELECT COUNT(*) FROM listener_play_history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(plays, 2, "the listening is copied");
        // The library is what Sampo can rebuild, and copying it would make the
        // snapshot too big to take often -- which is how backups stop happening.
        let has_files: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='files'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_files, 0, "the derived library is not copied");
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// A backup that vanishes when a table is missing is worse than useless.
    #[test]
    fn a_missing_table_is_skipped_rather_than_fatal() {
        let tmp = std::env::temp_dir().join(format!("vaino-bk2-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let db = library(&tmp);           // has no listener_likes at all
        let out = snapshot(&db).expect("a partial schema still backs up");
        assert!(out.exists());
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Rotation has to bite, or the directory grows without limit.
    #[test]
    fn only_the_newest_snapshots_are_kept() {
        let tmp = std::env::temp_dir().join(format!("vaino-bk3-{}", std::process::id()));
        let dir = tmp.join("listener-backups");
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..KEEP + 3 {
            std::fs::write(dir.join(format!("listener-{i:04}.db")), b"x").unwrap();
        }
        prune(&dir);
        let left = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(left, KEEP, "the oldest are dropped");
        // ...and it is the OLDEST that go: the newest name must survive.
        assert!(dir.join(format!("listener-{:04}.db", KEEP + 2)).exists());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
