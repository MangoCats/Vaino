//! Starting and replenishing a listening session.
//!
//! Every binary that plays from the library needs the same three things: the
//! library open, the resume point recovered, and the queue kept full. Written
//! once here so `station` and `vaino` cannot drift apart on what "start
//! playing" means — they differ only in whether a browser is watching.

use std::path::Path;

use crate::db::{DbError, Library, PlayerStore};
use crate::engine::Engine;

pub struct Session {
    pub lib: Library,
    store: Option<PlayerStore>,
    /// Where the saved passage left off, if there was one `[REQ-AUD-140]`.
    pub resume_ms: u64,
    resume_id: Option<i64>,
    depth: usize,
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
        Ok(Self { lib, store, resume_ms, resume_id, depth })
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
    pub fn refill(&self, engine: &mut Engine) {
        let short = engine.shortfall();
        if short == 0 {
            return;
        }
        match self.lib.random_radio(short) {
            Ok(entries) => entries.into_iter().for_each(|e| engine.enqueue(e)),
            Err(e) => eprintln!("refill: {e}"),
        }
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}
