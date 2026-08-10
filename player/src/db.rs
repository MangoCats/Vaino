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

use crate::queue::QueueEntry;

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
const SELECT: &str = "SELECT p.passage_id, f.path, p.start_ms, p.end_ms, \
                      p.lead_in_ms, p.lead_out_ms, p.gain_db \
                      FROM passages p JOIN files f USING (file_id)";

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueueEntry> {
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
    })
}

impl Library {
    /// Open read-only. The player never writes the library; only Sampo does,
    /// and enforcing that here means a bug cannot corrupt it.
    pub fn open(path: &std::path::Path) -> Result<Self, DbError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| DbError::Open(e.to_string()))?;
        Ok(Self { conn })
    }

    pub fn passage(&self, passage_id: i64) -> Result<QueueEntry, DbError> {
        self.conn
            .query_row(&format!("{SELECT} WHERE p.passage_id = ?1"), [passage_id], row_to_entry)
            .map_err(|e| DbError::Query(e.to_string()))
    }

    /// Radio passages in random order — a stand-in until the Program Director
    /// is wired in `[SPEC009]`. Radio only, per `[REQ-PD-120]`.
    pub fn random_radio(&self, limit: usize) -> Result<Vec<QueueEntry>, DbError> {
        let sql = format!("{SELECT} WHERE p.kind = 'radio' ORDER BY RANDOM() LIMIT ?1");
        let mut stmt = self.conn.prepare(&sql).map_err(|e| DbError::Query(e.to_string()))?;
        let rows = stmt
            .query_map([limit as i64], row_to_entry)
            .map_err(|e| DbError::Query(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| DbError::Query(e.to_string()))
    }

    pub fn count_radio(&self) -> Result<i64, DbError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM passages WHERE kind = 'radio'", [], |r| r.get(0))
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
             INSERT INTO files VALUES (1,'md5','/m/a.mp3',1,1.0,'mp3',300000,'t','t');
             INSERT INTO passages VALUES (1,1,'album',0,300000,NULL,NULL,NULL,'src');
             INSERT INTO passages VALUES (2,1,'radio',1200,298000,3000,4000,-2.5,'src');",
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

    #[test]
    fn a_null_lead_yields_a_gapless_handover() {
        use crate::queue::overlap_ms;
        let lib = Library { conn: fixture() };
        let album = lib.passage(1).unwrap();
        let radio = lib.passage(2).unwrap();
        assert_eq!(overlap_ms(&album, &radio), 0, "unanalysed passage must not crossfade");
    }
}
