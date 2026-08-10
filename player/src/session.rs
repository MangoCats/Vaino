//! Starting and replenishing a listening session.
//!
//! Every binary that plays from the library needs the same three things: the
//! library open, the resume point recovered, and the queue kept full. Written
//! once here so `station` and `vaino` cannot drift apart on what "start
//! playing" means — they differ only in whether a browser is watching.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::db::{DbError, Library, PlayerStore};
use crate::director::library::{Director, Explanation, Rng};
use crate::engine::Engine;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Why each recently chosen passage was chosen, shared with the web UI.
///
/// The engine thread writes, the server reads. Kept in memory rather than read
/// back from the database on every push: the answer is already in hand at the
/// moment of choosing, and a request path that re-queries for it would be a
/// second source of truth for the same fact.
pub type Explanations = Arc<Mutex<ExplanationLog>>;

/// Bounded on purpose. Only the queue and what is playing can be asked about,
/// so a handful is plenty and an unbounded map would grow for the life of the
/// process.
const KEEP_EXPLANATIONS: usize = 32;

#[derive(Default)]
pub struct ExplanationLog {
    by_passage: HashMap<i64, Explanation>,
    order: VecDeque<i64>,
}

impl ExplanationLog {
    pub fn get(&self, passage_id: i64) -> Option<&Explanation> {
        self.by_passage.get(&passage_id)
    }
    fn insert(&mut self, why: Explanation) {
        let id = why.passage_id;
        if self.by_passage.insert(id, why).is_none() {
            self.order.push_back(id);
        }
        while self.order.len() > KEEP_EXPLANATIONS {
            if let Some(old) = self.order.pop_front() {
                self.by_passage.remove(&old);
            }
        }
    }
}

pub struct Session {
    pub lib: Library,
    store: Option<PlayerStore>,
    /// Where the saved passage left off, if there was one `[REQ-AUD-140]`.
    pub resume_ms: u64,
    resume_id: Option<i64>,
    depth: usize,
    director: Option<Director>,
    rng: Rng,
    /// A second connection, because `prime` hands the resume store to the
    /// engine. Two handles on one SQLite file is the cheaper answer than
    /// sharing one across a thread boundary for a write this rare.
    decisions: Option<PlayerStore>,
    explanations: Explanations,
}

impl Session {
    /// `depth` is how many passages to keep queued ahead.
    pub fn open(db: &Path, depth: usize) -> Result<Self, DbError> {
        let lib = Library::open(db)?;
        // A resume point that cannot be opened is a first run, not a failure:
        // playback must never be blocked by the loss of a convenience.
        let store = PlayerStore::open(db)
            .map_err(|e| eprintln!("resume state unavailable ({e}); continuing without it"))
            .ok();
        let saved = store.as_ref().and_then(|s| s.load().ok()).flatten();
        let (resume_id, resume_ms) = match saved {
            Some((Some(id), pos, _)) => (Some(id), pos),
            _ => (None, 0),
        };
        // Selection degrades rather than fails: a library without the Program
        // Director's tables still plays, just uniformly at random.
        let director = match lib.director() {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("program director unavailable ({e}); selecting at random");
                None
            }
        };
        Ok(Self {
            lib,
            store,
            resume_ms,
            resume_id,
            depth,
            director,
            rng: Rng::from_clock(),
            decisions: PlayerStore::open(db).ok(),
            explanations: Explanations::default(),
        })
    }

    /// Hand the engine its store, its resume offset, and a full queue.
    pub fn prime(&mut self, engine: &mut Engine) {
        if let Some(s) = self.store.take() {
            engine.attach_store(s);
        }
        if let Some(id) = self.resume_id.take() {
            match self.lib.passage(id) {
                Ok(e) => {
                    println!("resuming passage {id} at {:.1}s", self.resume_ms as f64 / 1000.0);
                    engine.resume_at(self.resume_ms);
                    engine.enqueue(e);
                }
                // The library was rebuilt and the passage renumbered away.
                Err(_) => eprintln!("saved passage {id} is no longer in the library"),
            }
        }
        self.refill(engine);
    }

    /// Top the queue back up to `depth`. Called every tick in a continuous
    /// station; a no-op when the queue is already full, so it is cheap enough
    /// to call unconditionally rather than guessing when it is needed.
    ///
    /// Picks one at a time, telling the Director about each as it goes. Asking
    /// for five at once would weigh all five against the same stale history and
    /// could queue five tracks by one artist `[SPEC-DIR-115]`.
    pub fn refill(&mut self, engine: &mut Engine) {
        let short = engine.shortfall();
        if short == 0 {
            return;
        }
        let now = unix_now();
        let queued: Vec<i64> = engine.queued().map(|e| e.passage_id).collect();
        let mut chosen: Vec<i64> = queued;

        if let Some(d) = &mut self.director {
            for _ in 0..short {
                let Some(decision) = d.decide(now, &mut self.rng, &chosen) else {
                    // Everything eligible is blocked. Falling back keeps the
                    // radio playing, which [REQ-PD-100] requires; silence would
                    // be a worse answer than a repeat.
                    break;
                };
                let entry = decision.entry;
                d.note_queued(entry.passage_id, now);
                chosen.push(entry.passage_id);

                // Recording the reasoning must never be able to stop the
                // music, so both sinks are best-effort [SPEC-DIR-190].
                if let Some(store) = &self.decisions {
                    match serde_json::to_string(&decision.why) {
                        Ok(json) => {
                            if let Err(e) = store.record_decision(now, entry.passage_id, &json) {
                                eprintln!("record decision: {e}");
                            }
                        }
                        Err(e) => eprintln!("encode decision: {e}"),
                    }
                }
                if let Ok(mut log) = self.explanations.lock() {
                    log.insert(decision.why);
                }
                engine.enqueue(entry);
            }
        }

        let still_short = engine.shortfall();
        if still_short > 0 {
            match self.lib.random_radio(still_short) {
                Ok(entries) => entries.into_iter().for_each(|e| engine.enqueue(e)),
                Err(e) => eprintln!("refill: {e}"),
            }
        }
    }

    /// How the pool looks right now — for the panel, and for diagnosing a
    /// station that has gone quiet `[SPEC-DIR-190]`.
    pub fn census(&self) -> Option<crate::director::library::Census> {
        self.director.as_ref().map(|d| d.census(unix_now()))
    }

    /// Share the reasoning with a UI. Cloning the handle, not the data.
    pub fn explanations(&self) -> Explanations {
        Arc::clone(&self.explanations)
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}
