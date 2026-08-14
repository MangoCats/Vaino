//! Programmes and their seeds `[SPEC-DIR-140]`, `[SPEC-DIR-180]`,
//! `[REQ-PD-140]`.
//!
//! A programme is a list of exemplar passages, not a set of tuned parameters.
//! Naming songs beats moving sliders, and it is the mechanism MuLibPlay's user
//! actually exercised — eight programmes, 49 seeds, over six years.
//!
//! The active programme is whichever most recently started. A **hard switch**,
//! deliberately: blending was rejected because the flow stage already smooths
//! transitions `[SPEC-DIR-160]` and blending would make "which programme am I
//! in?" ambiguous for a tunable nobody asked for.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::db::DbError;

/// `[SPEC-DIR-195]`, proven.
pub const MAX_SEEDS: usize = 5;

const MINUTES_PER_DAY: i64 = 24 * 60;

#[derive(Debug, Clone)]
pub struct Program {
    pub id: i64,
    pub name: String,
    /// Minutes past local midnight.
    pub start_minute: i64,
}

/// `HH:MM` to minutes past midnight. Returns `None` for anything unparseable,
/// so a malformed row drops out rather than silently becoming midnight and
/// winning every comparison.
fn parse_hhmm(s: &str) -> Option<i64> {
    let (h, m) = s.split_once(':')?;
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

pub struct Programs {
    programs: Vec<Program>,
    seeds: HashMap<i64, Vec<String>>,
    /// Minutes to add to UTC to get local wall-clock time.
    ///
    /// Programme times are wall-clock — a 22:00 "Mellow" means ten at night
    /// where the listener is, not in Greenwich. There is no timezone in `std`,
    /// so the appliance stores its offset rather than the player guessing.
    pub utc_offset_minutes: i64,
    /// Set by the listener, overriding the clock until they revert
    /// `[SPEC-DIR-185]`.
    manual: Option<i64>,
}

impl Programs {
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }
    pub fn len(&self) -> usize {
        self.programs.len()
    }
    pub fn all(&self) -> &[Program] {
        &self.programs
    }
    pub fn get(&self, id: i64) -> Option<&Program> {
        self.programs.iter().find(|p| p.id == id)
    }

    /// Choose a programme by hand, overriding time of day `[SPEC-DIR-185]`.
    pub fn set_manual(&mut self, id: Option<i64>) {
        self.manual = id;
    }
    pub fn manual(&self) -> Option<i64> {
        self.manual
    }

    /// The programme in force at `now` (unix seconds).
    ///
    /// Whichever started most recently, wrapping through midnight — before the
    /// first start time of the day, the one that began last night is still
    /// running.
    pub fn active(&self, now: i64) -> Option<&Program> {
        if let Some(id) = self.manual {
            if let Some(p) = self.get(id) {
                return Some(p);
            }
        }
        if self.programs.is_empty() {
            return None;
        }
        let local = (now / 60 + self.utc_offset_minutes).rem_euclid(MINUTES_PER_DAY);
        // Largest start <= now, else the latest of the day (yesterday's).
        self.programs
            .iter()
            .filter(|p| p.start_minute <= local)
            .max_by_key(|p| p.start_minute)
            .or_else(|| self.programs.iter().max_by_key(|p| p.start_minute))
    }

    /// The seeds of a programme, down-selected `[SPEC-DIR-140]`.
    ///
    /// One per artist, preferring the least-recently-played, then capped at
    /// `MAX_SEEDS`. Preferring the least-recently-played keeps a programme from
    /// being defined by whatever it happened to play an hour ago, and makes the
    /// target drift slowly around the exemplars rather than sitting still.
    pub fn seeds_for(
        &self,
        program_id: i64,
        artist_of: &HashMap<String, String>,
        last_played: &HashMap<String, i64>,
    ) -> Vec<String> {
        let Some(all) = self.seeds.get(&program_id) else { return Vec::new() };

        // Ordering is by (last played, mbid). The mbid breaks ties so the
        // choice is stable: two never-played seeds must not swap between runs.
        let key = |m: &String| (last_played.get(m).copied().unwrap_or(i64::MIN), m.clone());

        let mut best: HashMap<&str, &String> = HashMap::new();
        let mut unattributed: Vec<&String> = Vec::new();
        for m in all {
            match artist_of.get(m) {
                Some(a) => {
                    let e = best.entry(a.as_str()).or_insert(m);
                    if key(m) < key(e) {
                        *e = m;
                    }
                }
                // No known artist: it cannot collide with anyone, so it stands
                // on its own rather than being grouped under a shared "unknown".
                None => unattributed.push(m),
            }
        }
        let mut chosen: Vec<&String> = best.into_values().chain(unattributed).collect();
        chosen.sort_by_key(|m| key(m));
        chosen.truncate(MAX_SEEDS);
        chosen.into_iter().cloned().collect()
    }

    pub fn load(conn: &Connection) -> Result<Self, DbError> {
        let mut programs = Vec::new();
        if let Ok(mut stmt) =
            conn.prepare("SELECT program_id, name, start_time FROM listener_programs")
        {
            if let Ok(rows) = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            }) {
                for (id, name, start) in rows.flatten() {
                    if let Some(start_minute) = parse_hhmm(&start) {
                        programs.push(Program { id, name, start_minute });
                    }
                }
            }
        }

        let mut seeds: HashMap<i64, Vec<String>> = HashMap::new();
        if let Ok(mut stmt) = conn
            .prepare("SELECT program_id, mbid FROM listener_program_seeds ORDER BY position")
        {
            if let Ok(rows) = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            {
                for (id, mbid) in rows.flatten() {
                    seeds.entry(id).or_default().push(mbid);
                }
            }
        }

        let utc_offset_minutes = conn
            .query_row("SELECT utc_offset_minutes FROM listener_settings WHERE id = 1", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0)
            .clamp(-(MINUTES_PER_DAY), MINUTES_PER_DAY);

        Ok(Self { programs, seeds, utc_offset_minutes, manual: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE listener_programs (program_id INTEGER, name TEXT, start_time TEXT);
             CREATE TABLE listener_program_seeds (program_id INTEGER, mbid TEXT, position INTEGER);
             CREATE TABLE listener_settings (id INTEGER PRIMARY KEY, utc_offset_minutes INTEGER);
             INSERT INTO listener_programs VALUES (1,'Soft','04:00'),(2,'Light','10:00'),
                 (3,'Cool','12:00'),(4,'Mellow','22:00'),(5,'Broken','not a time');
             INSERT INTO listener_program_seeds VALUES
                 (1,'a',0),(1,'b',1),(1,'c',2),(1,'d',3),(1,'e',4),(1,'f',5),(1,'g',6);",
        )
        .unwrap();
        c
    }

    /// Unix 0 is a Thursday midnight UTC.
    fn at(hour: i64, minute: i64) -> i64 {
        (hour * 60 + minute) * 60
    }

    #[test]
    fn a_malformed_start_time_drops_the_programme() {
        let p = Programs::load(&fixture()).unwrap();
        assert_eq!(p.len(), 4, "'not a time' must not become midnight and win every comparison");
    }

    #[test]
    fn the_active_programme_is_the_one_that_started_most_recently() {
        let p = Programs::load(&fixture()).unwrap();
        assert_eq!(p.active(at(4, 0)).unwrap().name, "Soft", "exactly at its start");
        assert_eq!(p.active(at(9, 59)).unwrap().name, "Soft");
        assert_eq!(p.active(at(10, 0)).unwrap().name, "Light");
        assert_eq!(p.active(at(13, 30)).unwrap().name, "Cool");
        assert_eq!(p.active(at(23, 59)).unwrap().name, "Mellow");
    }

    /// Before the first start of the day, last night's programme is still on.
    #[test]
    fn the_small_hours_belong_to_yesterdays_programme() {
        let p = Programs::load(&fixture()).unwrap();
        assert_eq!(p.active(at(2, 0)).unwrap().name, "Mellow", "22:00 is still running at 02:00");
        assert_eq!(p.active(at(0, 0)).unwrap().name, "Mellow");
    }

    /// Programme times are wall-clock, so the offset must move them.
    #[test]
    fn the_utc_offset_shifts_the_schedule() {
        let c = fixture();
        c.execute("INSERT INTO listener_settings VALUES (1, -300)", []).unwrap(); // UTC-5
        let p = Programs::load(&c).unwrap();
        assert_eq!(p.utc_offset_minutes, -300);
        // 15:00 UTC is 10:00 local, so Light has just begun.
        assert_eq!(p.active(at(15, 0)).unwrap().name, "Light");
        assert_eq!(p.active(at(14, 59)).unwrap().name, "Soft");
    }

    #[test]
    fn a_manual_choice_overrides_the_clock() {
        let mut p = Programs::load(&fixture()).unwrap();
        assert_eq!(p.active(at(13, 0)).unwrap().name, "Cool");
        p.set_manual(Some(4));
        assert_eq!(p.active(at(13, 0)).unwrap().name, "Mellow", "manual outranks time of day");
        p.set_manual(None);
        assert_eq!(p.active(at(13, 0)).unwrap().name, "Cool", "and reverting restores it");
    }

    /// A manual id that no longer exists must fall back to the clock rather
    /// than leaving the station with no programme at all.
    #[test]
    fn a_stale_manual_choice_falls_back_to_the_clock() {
        let mut p = Programs::load(&fixture()).unwrap();
        p.set_manual(Some(999));
        assert_eq!(p.active(at(13, 0)).unwrap().name, "Cool");
    }

    #[test]
    fn seeds_are_capped_and_one_per_artist() {
        let p = Programs::load(&fixture()).unwrap();
        let mut artist = HashMap::new();
        // a and b share an artist; the rest are distinct
        for (m, a) in [("a", "x"), ("b", "x"), ("c", "y"), ("d", "z"), ("e", "w"), ("f", "v"), ("g", "u")] {
            artist.insert(m.to_string(), a.to_string());
        }
        let mut played = HashMap::new();
        played.insert("a".to_string(), 100i64); // a played recently, b never
        let seeds = p.seeds_for(1, &artist, &played);
        assert_eq!(seeds.len(), MAX_SEEDS, "capped at max_seeds");
        assert!(seeds.contains(&"b".to_string()), "the least-recently-played of the pair");
        assert!(!seeds.contains(&"a".to_string()), "only one per artist");
    }

    /// Recently played seeds are the ones dropped by the cap, so the target
    /// drifts around the exemplars instead of sitting on whatever just played.
    #[test]
    fn the_cap_drops_the_most_recently_played() {
        let p = Programs::load(&fixture()).unwrap();
        let artist: HashMap<String, String> =
            ["a", "b", "c", "d", "e", "f", "g"].iter().map(|m| (m.to_string(), m.to_string())).collect();
        let played: HashMap<String, i64> =
            [("a", 900i64), ("b", 800), ("c", 700)].iter().map(|(m, t)| (m.to_string(), *t)).collect();
        let seeds = p.seeds_for(1, &artist, &played);
        assert_eq!(seeds.len(), MAX_SEEDS);
        assert!(!seeds.contains(&"a".to_string()), "the most recently played is dropped first");
    }

    #[test]
    fn seed_selection_is_stable_across_runs() {
        let p = Programs::load(&fixture()).unwrap();
        let artist: HashMap<String, String> =
            ["a", "b", "c", "d", "e", "f", "g"].iter().map(|m| (m.to_string(), m.to_string())).collect();
        let played = HashMap::new(); // none played: every key ties on time
        let first = p.seeds_for(1, &artist, &played);
        for _ in 0..5 {
            assert_eq!(p.seeds_for(1, &artist, &played), first, "ties must break deterministically");
        }
    }

    #[test]
    fn an_unknown_programme_has_no_seeds() {
        let p = Programs::load(&fixture()).unwrap();
        assert!(p.seeds_for(99, &HashMap::new(), &HashMap::new()).is_empty());
    }
}
