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

/// How far back each tier reaches `[REQ-LIB-160]`.
///
/// Grandfather-father-son, because the value of an old snapshot is not that it
/// is old but that it predates whatever went wrong. Damage noticed the same
/// afternoon needs yesterday; damage noticed at Christmas needs March; a
/// preference quietly corrupted two years ago needs a copy from before it.
///
/// At 2.4 MB apiece the whole ladder is a few hundred megabytes after a
/// decade, which is less than the library it protects.
pub const KEEP_DAYS: usize = 7;
pub const KEEP_MONTHS: usize = 12;

/// Calendar year, month and day for a Unix timestamp, UTC.
///
/// Written out rather than pulled in: the player has no date dependency and
/// this is the only thing that ever needed one. Howard Hinnant's civil-from-
/// days, which is exact for every date this will ever see -- no approximation
/// by 365.25, which drifts a day every century and would silently file a
/// snapshot under the wrong year.
fn civil(secs: i64) -> (i64, u32, u32) {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

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

/// Thin the snapshots to the retention ladder.
///
/// One per day for the last week, one per month for the last year, one per
/// year for ever, and always the newest whatever else happens. Within a period
/// the LATEST is kept: it is the one holding the most listening.
///
/// Failure is deliberately silent. An undeletable old snapshot is untidy, and
/// refusing to take new ones over it would turn tidiness into data loss.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut snaps: Vec<(i64, PathBuf)> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter_map(|p| Some((stamp_of(&p)?, p)))
        .collect();
    if snaps.is_empty() {
        return;
    }
    // Newest first, so the first seen in any period is the one to keep.
    snaps.sort_by(|a, b| b.0.cmp(&a.0));

    let mut keep: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    keep.insert(snaps[0].1.clone());          // the newest, always

    let mut days: Vec<(i64, u32, u32)> = Vec::new();
    let mut months: Vec<(i64, u32)> = Vec::new();
    let mut years: Vec<i64> = Vec::new();
    for (secs, path) in &snaps {
        let (y, m, d) = civil(*secs);
        if !days.contains(&(y, m, d)) && days.len() < KEEP_DAYS {
            days.push((y, m, d));
            keep.insert(path.clone());
        }
        if !months.contains(&(y, m)) && months.len() < KEEP_MONTHS {
            months.push((y, m));
            keep.insert(path.clone());
        }
        // Yearly has no limit: a decade of them is ten files.
        if !years.contains(&y) {
            years.push(y);
            keep.insert(path.clone());
        }
    }
    for (_, path) in &snaps {
        if !keep.contains(path) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The timestamp a snapshot's name carries, or `None` if it is not one.
fn stamp_of(path: &Path) -> Option<i64> {
    let name = path.file_name()?.to_str()?;
    let rest = name.strip_prefix("listener-")?.strip_suffix(".db")?;
    rest.parse().ok()
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

    /// The date arithmetic has to be exact, because a snapshot filed under the
    /// wrong year is one that survives when it should go, or goes when it is
    /// the only copy of that year.
    #[test]
    fn civil_dates_are_exact_at_the_awkward_boundaries() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(86_399), (1970, 1, 1), "one second before midnight");
        assert_eq!(civil(86_400), (1970, 1, 2));
        assert_eq!(civil(951_782_400), (2000, 2, 29), "a leap day in a leap century");
        assert_eq!(civil(1_709_164_800), (2024, 2, 29));
        assert_eq!(civil(1_735_689_599), (2024, 12, 31), "one second before new year");
        assert_eq!(civil(1_735_689_600), (2025, 1, 1));
    }

    /// Three years of hourly snapshots must thin to a ladder, not a pile.
    #[test]
    fn retention_keeps_a_day_a_month_and_a_year() {
        let tmp = std::env::temp_dir().join(format!("vaino-bk3-{}", std::process::id()));
        let dir = tmp.join("listener-backups");
        std::fs::create_dir_all(&dir).unwrap();

        // Every six hours for three years, ending "now". Six-hourly rather
        // than hourly only to keep the test quick: it exercises every tier
        // identically and writes a quarter of the files.
        let now = 1_800_000_000i64;
        let mut made = 0;
        for h in 0..(4 * 365 * 3) {
            let t = now - h * 21_600;
            std::fs::write(dir.join(format!("listener-{t}.db")), b"x").unwrap();
            made += 1;
        }
        prune(&dir);

        let left: Vec<i64> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| stamp_of(&e.ok()?.path()))
            .collect();
        // Seven days + twelve months + four calendar years, minus the overlaps
        // where one file satisfies several tiers at once.
        assert!(made > 4_000, "the pile really was a pile");
        assert!(left.len() <= KEEP_DAYS + KEEP_MONTHS + 5,
                "thinned to a ladder, got {}", left.len());
        assert!(left.len() >= KEEP_DAYS, "and the recent week survives: {}", left.len());
        assert!(left.contains(&now), "the newest is always kept");

        // A snapshot from each of the three years must remain, or "one per
        // year indefinitely" is not what happened.
        let years: std::collections::HashSet<i64> =
            left.iter().map(|t| civil(*t).0).collect();
        assert!(years.len() >= 3, "one per year survives: {years:?}");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
