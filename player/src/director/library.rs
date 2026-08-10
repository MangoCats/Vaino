//! Loading what Stage A needs, and choosing from it.
//!
//! [`frequency`](super::frequency) is pure arithmetic and stays that way. This
//! module is the only place that knows those numbers live in SQLite, which is
//! what lets the whole selection rule be tested without a database.
//!
//! Everything is loaded once and held in memory. The library is 8,079 radio
//! passages with ~37,000 plays behind it — a few megabytes — and selection then
//! costs no I/O at all, which matters on a Pi where the queue is topped up
//! every few seconds `[REQ-HW-140]`.

use std::collections::HashMap;

use rusqlite::Connection;
use serde::Serialize;

use super::frequency::{
    weigh, Candidate, Exclusion, Policy, Related, TimeScale, Tuning, Weighing,
};
use crate::db::{row_to_entry, DbError, COLS, FROM};
use crate::queue::QueueEntry;

/// A radio passage and the identity Stage A weighs it by.
struct Row {
    entry: QueueEntry,
    /// `None` when the passage has no identified recording. It can still play:
    /// an unidentified passage has no history, so it weighs as never-played
    /// rather than being silently excluded.
    mbid: Option<String>,
}

/// Deterministic, seedable, and dependency-free — SplitMix64.
///
/// Seedable matters more than quality here: `[REQ-PD-110]` measures divergence
/// from MuLibPlay, and a comparison whose randomness cannot be replayed is not
/// a measurement. Ten lines is a fair price for a reproducible station.
pub struct Rng(u64);

impl Rng {
    pub fn seeded(seed: u64) -> Self {
        Self(seed)
    }
    /// Seeded from the clock, for ordinary listening.
    pub fn from_clock() -> Self {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Self(n)
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        // 53 bits: the most an f64 can represent exactly.
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// How many losing candidates the record keeps.
///
/// `[SPEC-DIR-190]` asks for "the runners-up that lost". All 8,000 of them is
/// not an explanation, it is a data dump; the heaviest few are what answer
/// "why not something else?".
const RUNNERS_UP: usize = 5;

/// A complete answer to "why this track?" `[REQ-VIS-100]`.
///
/// Every term separately, never just the product -- a single number cannot be
/// argued with, and arguing with it is the point.
#[derive(Debug, Clone, Serialize)]
pub struct Explanation {
    pub passage_id: i64,
    pub title: String,
    pub weight: f64,
    pub artist_weight: f64,
    pub artist_blocked: bool,
    pub track_restraint: f64,
    pub track_ramp: f64,
    pub related_damping: f64,
    pub length_bonus: f64,
    pub occasion: f64,
    /// Eligible candidates it beat.
    pub pool_size: usize,
    pub pool_weight: f64,
    /// This passage's share of the total weight, as a percentage — the honest
    /// statement of how likely it was, as opposed to how good it is.
    pub share_pct: f64,
    pub runners_up: Vec<RunnerUp>,
    /// Stages B and C have not run: there is no flavor shaping yet
    /// `[SPEC-FD-040]`. Stated in the record so a stored decision is not later
    /// mistaken for a shaped one.
    pub stages: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerUp {
    pub passage_id: i64,
    pub title: String,
    pub weight: f64,
}

/// A chosen passage and the reasoning behind it.
pub struct Decision {
    pub entry: QueueEntry,
    pub why: Explanation,
}

pub struct Director {
    rows: Vec<Row>,
    policy: Policy,
    track_tuning: HashMap<String, Tuning>,
    artist_tuning: HashMap<String, Tuning>,
    /// recording → its artist. One artist per recording, as migrated.
    artist_of: HashMap<String, String>,
    last_played: HashMap<String, i64>,
    artist_last_played: HashMap<String, i64>,
    relations: HashMap<String, Vec<(String, f64)>>,
}

impl Director {
    pub fn load(conn: &Connection) -> Result<Self, DbError> {
        let q = |e: rusqlite::Error| DbError::Query(e.to_string());

        let sql = format!(
            "SELECT {COLS}, pr.mbid {FROM} \
             LEFT JOIN passage_recordings pr USING (passage_id) \
             WHERE p.kind = 'radio'"
        );
        let mut stmt = conn.prepare(&sql).map_err(q)?;
        let rows = stmt
            .query_map([], |r| Ok(Row { entry: row_to_entry(r)?, mbid: r.get(7)? }))
            .map_err(q)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(q)?;
        drop(stmt);

        let mut track_tuning = HashMap::new();
        let mut artist_tuning = HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT subject_kind, subject_id, rotation, recovery, restraint \
                 FROM listener_preferences",
            )
            .map_err(q)?;
        let prefs = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<f64>>(2)?,
                    r.get::<_, Option<f64>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                ))
            })
            .map_err(q)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(q)?;
        drop(stmt);
        for (kind, id, rot, rec, res) in prefs {
            // A NULL column means "not tuned", which is not the same as zero:
            // it must fall back to the default, not suppress the subject.
            let (base, map) = match kind.as_str() {
                "artist" => (Tuning::artist_defaults(), &mut artist_tuning),
                _ => (Tuning::track_defaults(), &mut track_tuning),
            };
            map.insert(
                id,
                Tuning {
                    rotation: rot.unwrap_or(base.rotation),
                    recovery: rec.unwrap_or(base.recovery),
                    restraint: res.unwrap_or(base.restraint),
                },
            );
        }

        let artist_of: HashMap<String, String> = map_query(conn, "SELECT mbid, artist_mbid FROM recording_artists")?;

        let mut last_played = HashMap::new();
        let mut artist_last_played: HashMap<String, i64> = HashMap::new();
        let mut stmt = conn
            .prepare(
                "SELECT mbid, MAX(played_at) FROM listener_play_history \
                 WHERE mbid IS NOT NULL GROUP BY mbid",
            )
            .map_err(q)?;
        let plays = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
            .map_err(q)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(q)?;
        drop(stmt);
        for (mbid, at) in plays {
            if let Some(a) = artist_of.get(&mbid) {
                let e = artist_last_played.entry(a.clone()).or_insert(at);
                *e = (*e).max(at);
            }
            last_played.insert(mbid, at);
        }

        let mut relations: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        let mut stmt = conn
            .prepare("SELECT mbid, related_mbid, strength FROM recording_relations")
            .map_err(q)?;
        let rels = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?))
            })
            .map_err(q)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(q)?;
        drop(stmt);
        for (a, b, s) in rels {
            relations.entry(a).or_default().push((b, s));
        }

        // Absent settings are the defaults, not an error: a library that has
        // never been tuned must still play [SPEC-DIR-158].
        let scales = conn
            .query_row(
                "SELECT artist_time_scale, track_time_scale FROM listener_settings WHERE id = 1",
                [],
                |r| Ok((r.get::<_, f64>(0)?, r.get::<_, f64>(1)?)),
            )
            .ok();
        let policy = Policy {
            artist_scale: scales.map(|s| TimeScale::new(s.0)).unwrap_or_default(),
            track_scale: scales.map(|s| TimeScale::new(s.1)).unwrap_or_default(),
            ..Default::default()
        };

        Ok(Self {
            rows,
            policy,
            track_tuning,
            artist_tuning,
            artist_of,
            last_played,
            artist_last_played,
            relations,
        })
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }
    pub fn set_policy(&mut self, p: Policy) {
        self.policy = p;
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    fn age(&self, map: &HashMap<String, i64>, key: &str, now: i64) -> Option<f64> {
        // A play stamped in the future -- clock skew, or a restored backup --
        // must read as "just played", not as a negative age that would sail
        // through every block.
        map.get(key).map(|at| (now - at).max(0) as f64)
    }

    /// Weigh every radio passage. `now` is unix seconds, passed in so a
    /// selection can be replayed against a frozen history `[REQ-PD-110]`.
    pub fn weigh_all(&self, now: i64) -> Vec<(&QueueEntry, Weighing)> {
        let mut related_buf: Vec<Related> = Vec::new();
        self.rows
            .iter()
            .map(|row| {
                let mbid = row.mbid.as_deref();
                let track = mbid
                    .and_then(|m| self.track_tuning.get(m))
                    .copied()
                    .unwrap_or_else(Tuning::track_defaults);
                let artist_id = mbid.and_then(|m| self.artist_of.get(m));
                let artist = artist_id
                    .and_then(|a| self.artist_tuning.get(a))
                    .copied()
                    .unwrap_or_else(Tuning::artist_defaults);

                related_buf.clear();
                if let Some(m) = mbid {
                    if let Some(rel) = self.relations.get(m) {
                        for (other, strength) in rel {
                            related_buf.push(Related {
                                age_s: self.age(&self.last_played, other, now),
                                strength: *strength,
                            });
                        }
                    }
                }

                let c = Candidate {
                    length_s: row.entry.duration_ms() as f64 / 1000.0,
                    depth_s: row.entry.start_ms as f64 / 1000.0,
                    track,
                    artist,
                    track_age_s: mbid.and_then(|m| self.age(&self.last_played, m, now)),
                    artist_age_s: artist_id
                        .and_then(|a| self.age(&self.artist_last_played, a, now)),
                    related: &related_buf,
                    // Occasion curves are not implemented yet [SPEC-DIR-130];
                    // 1.0 is the correct neutral, not a placeholder to forget.
                    occasion: 1.0,
                };
                (&row.entry, weigh(&c, &self.policy))
            })
            .collect()
    }

    /// Mark a passage as just played, so the next pick sees it.
    ///
    /// Called when a passage is QUEUED, not when it finishes -- MuLibPlay's own
    /// note says the structures update "as each new track finishes playing (or
    /// is put in the play queue)". Without it, topping up five slots at once
    /// would happily queue five tracks by one artist, because every pick would
    /// weigh against the same stale history.
    pub fn note_queued(&mut self, passage_id: i64, at: i64) {
        let Some(mbid) = self
            .rows
            .iter()
            .find(|r| r.entry.passage_id == passage_id)
            .and_then(|r| r.mbid.clone())
        else {
            return;
        };
        if let Some(a) = self.artist_of.get(&mbid) {
            let a = a.clone();
            let e = self.artist_last_played.entry(a).or_insert(at);
            *e = (*e).max(at);
        }
        let e = self.last_played.entry(mbid).or_insert(at);
        *e = (*e).max(at);
    }

    /// Weighted-random pick over the eligible pool.
    ///
    /// This is Stage D applied directly to Stage A, with no flavor shaping in
    /// between: stages B and C need flavor distance `[SPEC-FD-040]`. It is an
    /// honest increment -- frequency alone already beats uniform random -- and
    /// the seam is exactly where the shaped pool will be inserted.
    pub fn choose(&self, now: i64, rng: &mut Rng) -> Option<QueueEntry> {
        self.decide(now, rng, &[]).map(|d| d.entry)
    }

    /// As [`Director::decide`], discarding the reasoning.
    pub fn choose_excluding(&self, now: i64, rng: &mut Rng, skip: &[i64]) -> Option<QueueEntry> {
        self.decide(now, rng, skip).map(|d| d.entry)
    }

    /// As [`Director::choose`], skipping passages already in the queue.
    ///
    /// [`Director::note_queued`] handles this for anything identified, by
    /// making it its own rotation block. This is the structural guarantee for
    /// the rest: an unidentified passage has no MBID to block on, and must
    /// still not appear twice in one queue.
    pub fn decide(&self, now: i64, rng: &mut Rng, skip: &[i64]) -> Option<Decision> {
        let weighed = self.weigh_all(now);
        let live = |(e, w): &(&QueueEntry, Weighing)| {
            w.is_eligible() && !skip.contains(&e.passage_id)
        };
        let total: f64 = weighed.iter().filter(|x| live(x)).map(|(_, w)| w.weight).sum();
        if !(total > 0.0) {
            return None;
        }
        let mut target = rng.unit() * total;
        let mut winner: Option<&(&QueueEntry, Weighing)> = None;
        for pair in &weighed {
            if !live(pair) {
                continue;
            }
            target -= pair.1.weight;
            if target <= 0.0 {
                winner = Some(pair);
                break;
            }
        }
        // Floating-point drift can leave a hair of `target` after the loop.
        // Taking the last eligible passage is correct, not a fallback.
        let (entry, w) = winner.or_else(|| weighed.iter().rev().find(|x| live(x)))?;

        // The heaviest losers, which is what "why not something else?" means.
        let mut rest: Vec<&(&QueueEntry, Weighing)> = weighed
            .iter()
            .filter(|x| live(x) && x.0.passage_id != entry.passage_id)
            .collect();
        rest.sort_by(|a, b| b.1.weight.partial_cmp(&a.1.weight).unwrap_or(std::cmp::Ordering::Equal));

        let pool_size = weighed.iter().filter(|x| live(x)).count();
        Some(Decision {
            entry: (*entry).clone(),
            why: Explanation {
                passage_id: entry.passage_id,
                title: entry.title(),
                weight: w.weight,
                artist_weight: w.artist_weight,
                artist_blocked: w.artist_blocked,
                track_restraint: w.track_restraint,
                track_ramp: w.track_ramp,
                related_damping: w.related_damping,
                length_bonus: w.length_bonus,
                occasion: w.occasion,
                pool_size,
                pool_weight: total,
                share_pct: if total > 0.0 { w.weight / total * 100.0 } else { 0.0 },
                runners_up: rest
                    .iter()
                    .take(RUNNERS_UP)
                    .map(|(e, rw)| RunnerUp {
                        passage_id: e.passage_id,
                        title: e.title(),
                        weight: rw.weight,
                    })
                    .collect(),
                stages: "frequency only; no flavor shaping yet [SPEC-FD-040]",
            },
        })
    }

    /// Why the pool is the size it is — for the panel, and for diagnosing a
    /// station that has gone quiet.
    pub fn census(&self, now: i64) -> Census {
        let mut c = Census::default();
        for (_, w) in self.weigh_all(now) {
            match w.excluded {
                None => {
                    c.eligible += 1;
                    c.total_weight += w.weight;
                }
                Some(Exclusion::ArtistRotationBlock) => c.artist_blocked += 1,
                Some(Exclusion::TrackRotationBlock) => c.track_blocked += 1,
                Some(Exclusion::RelatedRotationBlock) => c.related_blocked += 1,
                Some(Exclusion::BelowMinWeight) => c.below_min_weight += 1,
                Some(_) => c.filtered += 1,
            }
        }
        c
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Census {
    pub eligible: usize,
    pub artist_blocked: usize,
    pub track_blocked: usize,
    pub related_blocked: usize,
    pub below_min_weight: usize,
    pub filtered: usize,
    pub total_weight: f64,
}

fn map_query(conn: &Connection, sql: &str) -> Result<HashMap<String, String>, DbError> {
    let q = |e: rusqlite::Error| DbError::Query(e.to_string());
    let mut stmt = conn.prepare(sql).map_err(q)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(q)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(q)?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000;
    const DAY: i64 = 86_400;

    /// The slice of SPEC008 selection touches. Written out rather than loaded
    /// from schema.sql so these tests pin the column names the queries need.
    fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files (file_id INTEGER PRIMARY KEY, path TEXT NOT NULL);
             CREATE TABLE passages (passage_id INTEGER PRIMARY KEY, file_id INTEGER NOT NULL,
                 kind TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                 lead_in_ms INTEGER, lead_out_ms INTEGER, gain_db REAL);
             CREATE TABLE passage_recordings (passage_id INTEGER, mbid TEXT);
             CREATE TABLE recording_artists (mbid TEXT, artist_mbid TEXT);
             CREATE TABLE recording_relations (mbid TEXT, related_mbid TEXT, strength REAL);
             CREATE TABLE listener_preferences (subject_kind TEXT, subject_id TEXT,
                 rotation REAL, recovery REAL, restraint REAL);
             CREATE TABLE listener_play_history (play_id INTEGER PRIMARY KEY,
                 played_at INTEGER, passage_id INTEGER, mbid TEXT);
             CREATE TABLE listener_settings (id INTEGER PRIMARY KEY,
                 artist_time_scale REAL, track_time_scale REAL, updated_at TEXT);
             INSERT INTO files VALUES (1, '/m/a.mp3');
             -- three 180 s radio passages, one album passage that must never appear
             INSERT INTO passages VALUES (1,1,'radio',0,180000,0,0,0.0);
             INSERT INTO passages VALUES (2,1,'radio',0,180000,0,0,0.0);
             INSERT INTO passages VALUES (3,1,'radio',0,180000,0,0,0.0);
             INSERT INTO passages VALUES (4,1,'album',0,180000,0,0,0.0);
             INSERT INTO passage_recordings VALUES (1,'rec-a'),(2,'rec-b'),(3,'rec-c');
             INSERT INTO recording_artists VALUES ('rec-a','art-1'),('rec-b','art-2'),
                                                  ('rec-c','art-3');",
        )
        .unwrap();
        c
    }

    #[test]
    fn loads_only_radio_passages() {
        let d = Director::load(&fixture()).unwrap();
        assert_eq!(d.len(), 3, "the album passage must not be selectable [REQ-PD-120]");
    }

    /// With no history everything is eligible and equally weighted, so the
    /// station is uniform random rather than silently biased [SPEC-DIR-158].
    #[test]
    fn a_fresh_library_is_all_eligible() {
        let d = Director::load(&fixture()).unwrap();
        let c = d.census(NOW);
        assert_eq!(c.eligible, 3);
        assert!((c.total_weight - 3.0).abs() < 1e-9, "weights {}", c.total_weight);
    }

    #[test]
    fn a_recent_play_blocks_that_recording_and_its_artist() {
        let c = fixture();
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 1, 'rec-a')", [NOW - 60])
            .unwrap();
        let d = Director::load(&c).unwrap();
        let cen = d.census(NOW);
        assert_eq!(cen.eligible, 2, "the played recording drops out");
        assert_eq!(cen.artist_blocked + cen.track_blocked, 1);
    }

    /// The repair, end to end: a play of rec-b blocks rec-a through their
    /// relation, even though rec-a itself has never played.
    #[test]
    fn a_relation_blocks_across_recordings() {
        let c = fixture();
        c.execute("INSERT INTO recording_relations VALUES ('rec-a','rec-b',1.0)", [])
            .unwrap();
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 2, 'rec-b')", [NOW - 60])
            .unwrap();
        let d = Director::load(&c).unwrap();
        assert_eq!(d.census(NOW).related_blocked, 1, "rec-a must be blocked by rec-b");
    }

    /// Ages come from the history, so an old play must recover. Ten days is
    /// past the 4.2-day default rotation but inside the 16.6-day ramp.
    #[test]
    fn an_old_play_is_damped_rather_than_blocked() {
        let c = fixture();
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 1, 'rec-a')",
                  [NOW - 10 * DAY]).unwrap();
        let d = Director::load(&c).unwrap();
        let w = d.weigh_all(NOW);
        let (_, a) = w.iter().find(|(e, _)| e.passage_id == 1).unwrap();
        assert!(a.is_eligible(), "10 days is past the 4.2-day rotation: {:?}", a.excluded);
        assert!(a.track_ramp > 0.0 && a.track_ramp < 1.0, "mid-ramp, got {}", a.track_ramp);
    }

    /// A play stamped in the future -- clock skew, a restored backup -- must
    /// read as "just played", never as a negative age that clears every block.
    #[test]
    fn a_future_play_does_not_clear_the_block() {
        let c = fixture();
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 1, 'rec-a')",
                  [NOW + 30 * DAY]).unwrap();
        let d = Director::load(&c).unwrap();
        assert_eq!(d.census(NOW).eligible, 2, "the future play must still block");
    }

    #[test]
    fn tuning_is_read_and_nulls_fall_back_to_defaults() {
        let c = fixture();
        // rotation tuned to 3.0 (41 days), recovery left NULL
        c.execute("INSERT INTO listener_preferences VALUES ('recording','rec-a',3.0,NULL,NULL)", [])
            .unwrap();
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 1, 'rec-a')",
                  [NOW - 10 * DAY]).unwrap();
        let d = Director::load(&c).unwrap();
        let w = d.weigh_all(NOW);
        let (_, a) = w.iter().find(|(e, _)| e.passage_id == 1).unwrap();
        assert_eq!(a.excluded, Some(Exclusion::TrackRotationBlock),
                   "a 41-day rotation must still block a 10-day-old play");
    }

    #[test]
    fn time_scales_are_read_from_settings() {
        let c = fixture();
        c.execute("INSERT INTO listener_settings VALUES (1, 0.5, 0.25, 't')", []).unwrap();
        let d = Director::load(&c).unwrap();
        assert_eq!(d.policy().artist_scale.get(), 0.5);
        assert_eq!(d.policy().track_scale.get(), 0.25);
    }

    /// Halving the track scale must actually shorten a block, not merely load.
    #[test]
    fn a_scaled_block_expires_sooner() {
        let c = fixture();
        // 3 days: inside the 4.2-day default rotation, outside a halved one.
        c.execute("INSERT INTO listener_play_history VALUES (1, ?1, 1, 'rec-a')",
                  [NOW - 3 * DAY]).unwrap();
        assert_eq!(Director::load(&c).unwrap().census(NOW).track_blocked, 1);
        c.execute("INSERT INTO listener_settings VALUES (1, 1.0, 0.5, 't')", []).unwrap();
        assert_eq!(Director::load(&c).unwrap().census(NOW).track_blocked, 0,
                   "half the rotation means the block has expired");
    }

    /// The record must be able to answer "why not something else?", so the
    /// losers travel with the winner.
    #[test]
    fn a_decision_carries_its_decomposition_and_its_losers() {
        let d = Director::load(&fixture()).unwrap();
        let dec = d.decide(NOW, &mut Rng::seeded(11), &[]).unwrap();
        assert_eq!(dec.why.pool_size, 3);
        assert!((dec.why.pool_weight - 3.0).abs() < 1e-9);
        assert!((dec.why.share_pct - 33.333).abs() < 0.01, "share {}", dec.why.share_pct);
        assert_eq!(dec.why.runners_up.len(), 2, "the other two eligible passages");
        assert!(dec.why.runners_up.iter().all(|r| r.passage_id != dec.why.passage_id),
                "the winner must not be listed among those it beat");
        assert!(!dec.why.title.is_empty());
    }

    /// Runners-up are the HEAVIEST losers, not an arbitrary five.
    #[test]
    fn runners_up_are_ordered_by_weight() {
        let c = fixture();
        c.execute("INSERT INTO listener_preferences VALUES ('recording','rec-c',NULL,NULL,-1.0)", [])
            .unwrap();
        let d = Director::load(&c).unwrap();
        // rec-c is 10x; whenever it does not win it must head the losers.
        for seed in 0..20 {
            let dec = d.decide(NOW, &mut Rng::seeded(seed), &[]).unwrap();
            if dec.why.passage_id != 3 {
                assert_eq!(dec.why.runners_up[0].passage_id, 3,
                           "the heaviest loser must be listed first");
            }
        }
    }

    #[test]
    fn selection_is_reproducible_from_a_seed() {
        let d = Director::load(&fixture()).unwrap();
        let a: Vec<_> = (0..8).map(|_| d.choose(NOW, &mut Rng::seeded(42)).unwrap().passage_id).collect();
        let b: Vec<_> = (0..8).map(|_| d.choose(NOW, &mut Rng::seeded(42)).unwrap().passage_id).collect();
        assert_eq!(a, b, "the same seed must replay the same station [REQ-PD-110]");
    }

    /// Weight must actually steer the roulette, or Stage A is decorative.
    #[test]
    fn a_heavier_passage_wins_more_often() {
        let c = fixture();
        // restraint -1.0 on rec-a is a 10x boost; the others stay at 1.0
        c.execute("INSERT INTO listener_preferences VALUES ('recording','rec-a',NULL,NULL,-1.0)", [])
            .unwrap();
        let d = Director::load(&c).unwrap();
        let mut rng = Rng::seeded(7);
        let mut wins = 0;
        for _ in 0..1000 {
            if d.choose(NOW, &mut rng).unwrap().passage_id == 1 {
                wins += 1;
            }
        }
        // 10 / 12 == 83%. Wide bounds: this asserts steering, not calibration.
        assert!((750..=900).contains(&wins), "heavy passage won {wins}/1000");
    }

    /// Queueing must feed back into selection, or a five-slot top-up could
    /// queue five tracks by one artist.
    #[test]
    fn queueing_a_passage_blocks_it_for_the_next_pick() {
        let mut d = Director::load(&fixture()).unwrap();
        assert_eq!(d.census(NOW).eligible, 3);
        d.note_queued(1, NOW);
        assert_eq!(d.census(NOW).eligible, 2, "a queued passage stops being a candidate");
    }

    #[test]
    fn the_skip_list_keeps_a_passage_out_of_its_own_queue() {
        let d = Director::load(&fixture()).unwrap();
        let mut rng = Rng::seeded(3);
        for _ in 0..50 {
            let got = d.choose_excluding(NOW, &mut rng, &[1, 2]).unwrap();
            assert_eq!(got.passage_id, 3, "only the unskipped passage may be chosen");
        }
    }

    #[test]
    fn an_entirely_blocked_library_chooses_nothing_rather_than_panicking() {
        let c = fixture();
        for (i, m) in [(1, "rec-a"), (2, "rec-b"), (3, "rec-c")] {
            c.execute("INSERT INTO listener_play_history VALUES (NULL, ?1, ?2, ?3)",
                      rusqlite::params![NOW - 60, i, m]).unwrap();
        }
        let d = Director::load(&c).unwrap();
        assert_eq!(d.census(NOW).eligible, 0);
        assert!(d.choose(NOW, &mut Rng::seeded(1)).is_none(),
                "no eligible passage must be None, so the caller can fall back");
    }

    #[test]
    fn an_unidentified_passage_still_plays() {
        let c = fixture();
        c.execute("DELETE FROM passage_recordings WHERE mbid = 'rec-a'", []).unwrap();
        let d = Director::load(&c).unwrap();
        assert_eq!(d.census(NOW).eligible, 3, "no MBID means no history, not exclusion");
    }
}
