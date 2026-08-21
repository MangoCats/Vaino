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

/// Programme selection, shared between the web thread and the engine thread.
///
/// The Director lives on the engine thread and is not `Sync`, so the browser
/// cannot reach it directly. It writes an intent here instead, and the engine
/// applies it on its next refill. One shared cell rather than another command
/// channel: the state is small, idempotent, and the browser needs to *read* it
/// back to show what is active.
#[derive(Default)]
pub struct Controls {
    /// Chosen by hand, overriding time of day until cleared `[SPEC-DIR-185]`.
    pub manual_program: Option<i64>,
    /// `(id, name, start_time)`, for the browser to offer.
    pub programs: Vec<(i64, String, String)>,
    /// The programme actually in force, as the engine last resolved it.
    pub active: Option<String>,
    /// Asks for a live Director rebuild, so music imported into the library
    /// becomes selectable without restarting the player `[IMPL-SUI-075]`.
    ///
    /// The same intent-cell pattern as `manual_program`, and for the same
    /// reason: the Director is not `Sync`, so the browser cannot reach it.
    pub reload_requested: bool,
    /// What the rebuild is doing, for the browser to show. Set by the engine.
    pub reload_status: Option<String>,
    /// The Director's pool as `(eligible, total)`, refreshed on adoption.
    ///
    /// Here so a rebuild's effect is **observable** rather than asserted:
    /// importing music and reloading moves `total`, and without a number to
    /// look at "it reloaded" would be a claim with nothing behind it
    /// `[GDE-CHT-030]`.
    pub pool: Option<(usize, usize)>,
}

pub type SharedControls = Arc<Mutex<Controls>>;

/// How much queued audio must be in hand before a live Director rebuild starts
/// `[IMPL-SUI-075]`.
///
/// Measured by `dircheck`: a rebuild over 8,330 radio passages takes **9.86 s
/// on the appliance** and 0.89 s on a desktop, so three minutes covers the slow
/// case eighteen times over. The margin is not really about time — the rebuild
/// is off the audio path and cannot glitch a note. It is about **I/O**: those
/// ten seconds are heavy SQLite reading from an SD card, and starting them only
/// when decode is well ahead keeps the two from contending for the same card.
///
/// The default depth of five passages holds far more than this, so in ordinary
/// running the rebuild starts at once; the threshold bites only when the queue
/// is short, which is exactly when the Director is needed for something else.
pub const RELOAD_MIN_QUEUE_MS: u64 = 180_000;

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
    /// Was it playing when it last stopped? `[PI5-PWR-030]`
    resume_playing: bool,
    depth: usize,
    director: Option<Director>,
    rng: Rng,
    /// A second connection, because `prime` hands the resume store to the
    /// engine. Two handles on one SQLite file is the cheaper answer than
    /// sharing one across a thread boundary for a write this rare.
    decisions: Option<PlayerStore>,
    explanations: Explanations,
    controls: SharedControls,
    /// What the Director was told about each queued passage, kept until the
    /// engine confirms it could be opened `[REQ-PD-112]`.
    ///
    /// Bounded by the queue: an entry goes when its passage is dropped, and
    /// the rest are pruned to what is still queued -- a passage that has been
    /// admitted can no longer fail to open, so its note is dead weight.
    notes: HashMap<i64, crate::director::library::QueuedNote>,
    /// The library file, so a rebuild can open its own connection
    /// `[IMPL-SUI-075]`. A path rather than a shared handle, for the reason
    /// `Ui` keeps one: `rusqlite`'s `Connection` is not `Sync`.
    db: std::path::PathBuf,
    /// A rebuild in flight. `Director` is `Send`, asserted at compile time in
    /// `dircheck`, so it is built on its own thread and handed back here —
    /// the running one keeps answering selections throughout, and there is
    /// never a window with none.
    rebuild: Option<std::sync::mpsc::Receiver<Result<Box<Director>, String>>>,
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
        // The saved play state is carried, not discarded. It was read and
        // thrown away here for as long as the row has existed, which is why an
        // appliance that lost power came back silent even though it had been
        // playing `[PI5-PWR-030]`.
        let (resume_id, resume_ms, resume_playing) = match saved {
            Some((Some(id), pos, playing)) => (Some(id), pos, playing),
            _ => (None, 0, false),
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
            resume_playing,
            depth,
            director,
            rng: Rng::from_clock(),
            decisions: PlayerStore::open(db).ok(),
            explanations: Explanations::default(),
            controls: SharedControls::default(),
            notes: HashMap::new(),
            db: db.to_path_buf(),
            rebuild: None,
        })
    }

    /// Start a rebuild when asked, and adopt one that has finished
    /// `[IMPL-SUI-075]`.
    ///
    /// Called before the shortfall check, because a rebuild is exactly what
    /// must **not** happen while the queue is short: the Director is needed to
    /// refill it, and the SD card is needed to decode from it.
    fn tend_rebuild(&mut self, engine: &Engine) {
        if let Some(rx) = &self.rebuild {
            match rx.try_recv() {
                Err(std::sync::mpsc::TryRecvError::Empty) => return, // still building
                Ok(Ok(fresh)) => {
                    self.adopt(*fresh, engine);
                    self.say_reload("rebuilt");
                }
                Ok(Err(e)) => self.say_reload(&format!("rebuild failed: {e}")),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The thread died without answering. The old Director is
                    // untouched and still selecting, so this costs nothing but
                    // the attempt -- which is the whole point of building a
                    // replacement rather than dropping the incumbent first.
                    self.say_reload("rebuild thread stopped without answering");
                }
            }
            self.rebuild = None;
            return;
        }

        let asked = match self.controls.lock() {
            Ok(mut c) => {
                // First time through, publish the pool so there is a number to
                // compare a rebuild against.
                if c.pool.is_none() {
                    drop(c);
                    self.publish_pool();
                    match self.controls.lock() {
                        Ok(mut c) => std::mem::take(&mut c.reload_requested),
                        Err(_) => false,
                    }
                } else {
                    std::mem::take(&mut c.reload_requested)
                }
            }
            Err(_) => false,
        };
        if !asked {
            return;
        }
        // Enough audio in hand, or a queue already as full as it will get --
        // waiting past that point would be waiting for something that is not
        // coming.
        let queued_ms: u64 = engine.queued().map(|e| e.end_ms.saturating_sub(e.start_ms)).sum();
        if queued_ms < RELOAD_MIN_QUEUE_MS && engine.shortfall() > 0 {
            self.say_reload(&format!(
                "waiting for {} s of queue before rebuilding ({} s in hand)",
                RELOAD_MIN_QUEUE_MS / 1000,
                queued_ms / 1000
            ));
            // Put the request back: it has not been served, only deferred.
            if let Ok(mut c) = self.controls.lock() {
                c.reload_requested = true;
            }
            return;
        }

        let path = self.db.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        match std::thread::Builder::new()
            .name("director-rebuild".into())
            .spawn(move || {
                let built = Library::open(&path)
                    .and_then(|l| l.director())
                    .map(Box::new)
                    .map_err(|e| format!("{e:?}"));
                let _ = tx.send(built);
            }) {
            Ok(_) => {
                self.rebuild = Some(rx);
                self.say_reload("rebuilding");
            }
            Err(e) => self.say_reload(&format!("could not start rebuild: {e}")),
        }
    }

    /// Swap a freshly built Director in, carrying the queue's bookkeeping over.
    ///
    /// The replacement's `last_played` comes from `listener_play_history`,
    /// which does not know about passages that are queued but have not played.
    /// Without re-noting them it would consider their recordings and artists
    /// un-suppressed and could pick a sibling that rotation had ruled out
    /// `[REQ-PD-112]`. The notes are rebuilt rather than moved because each one
    /// holds the *previous* value from the Director that issued it.
    fn adopt(&mut self, fresh: Director, engine: &Engine) {
        self.director = Some(fresh);
        self.notes.clear();
        let now = unix_now();
        let queued: Vec<i64> = engine.queued().map(|e| e.passage_id).collect();
        if let Some(d) = self.director.as_mut() {
            for id in queued {
                if let Some(note) = d.note_queued(id, now) {
                    self.notes.insert(id, note);
                }
            }
        }
        self.publish_pool();
    }

    /// Put the pool size where the browser can see it.
    fn publish_pool(&self) {
        let Some(c) = self.census() else { return };
        let total = c.eligible + c.artist_blocked + c.track_blocked + c.related_blocked
            + c.below_min_weight + c.filtered;
        if let Ok(mut ctl) = self.controls.lock() {
            ctl.pool = Some((c.eligible, total));
        }
    }

    fn say_reload(&self, what: &str) {
        if let Ok(mut c) = self.controls.lock() {
            c.reload_status = Some(what.to_string());
        }
    }

    /// Was the player playing when it last saved? `[PI5-PWR-030]`
    ///
    /// A caller decides what to do with that: `vaino` resumes playback, while
    /// `station` starts when told to and has no use for it.
    pub fn resume_playing(&self) -> bool {
        self.resume_playing
    }

    /// Name a passage before it is shown `[REQ-VIS-170]`.
    ///
    /// MusicBrainz first, then the file's own tags for whatever it did not
    /// answer -- which today is every album name, the release tables being
    /// empty until Sampo fills them. Done once per passage, on the way into the
    /// queue, rather than per render: it touches the disk, and a snapshot goes
    /// out twice a second.
    /// Takes the library rather than `&self` so it can be called while the
    /// Director holds a mutable borrow of its own field -- disjoint fields,
    /// which the compiler will allow only if the borrow is spelled out.
    fn describe(lib: &Library, e: &mut crate::queue::QueueEntry) {
        lib.describe(e);
        if e.naming.mb_title.is_none()
            || e.naming.mb_artist.is_none()
            || e.naming.mb_album.is_none()
        {
            // The scanned copy first: reading tags means probing the file, and
            // this runs on the way into the queue while music is playing
            // `[REQ-VIS-180]`. Falling back to the file keeps an unscanned
            // library working, just more slowly.
            let tags = lib
                .stored_tags(e.passage_id)
                .unwrap_or_else(|| crate::tags::read(&e.path));
            e.naming.apply_tags(tags);
        }
    }

    /// Hand the engine its store, its resume offset, and a full queue.
    pub fn prime(&mut self, engine: &mut Engine) {
        // Before the store is handed over, since it is the thing that holds
        // them: volume and the skip shape as they were last left
        // `[REQ-VIS-155]`.
        if let Some(saved) = self.store.as_ref().and_then(|s| s.load_settings()) {
            engine.apply_settings(&saved);
        }
        if let Some(s) = self.store.take() {
            engine.attach_store(s);
        }
        if let Some(id) = self.resume_id.take() {
            match self.lib.passage(id) {
                Ok(mut e) => {
                    Self::describe(&self.lib, &mut e);
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
        // Before anything else, and deliberately before the shortfall check:
        // a rebuild must not start while the queue is short `[IMPL-SUI-075]`.
        self.tend_rebuild(engine);

        // Apply the browser's programme choice before selecting, and report
        // back what is actually in force -- "auto" resolves to a name only the
        // Director can supply.
        let now = unix_now();
        if let (Some(d), Ok(mut c)) = (&mut self.director, self.controls.lock()) {
            if c.programs.is_empty() {
                c.programs = d
                    .programs()
                    .all()
                    .iter()
                    .map(|p| (p.id, p.name.clone(), format!("{:02}:{:02}", p.start_minute / 60, p.start_minute % 60)))
                    .collect();
            }
            if d.programs().manual() != c.manual_program {
                d.programs_mut().set_manual(c.manual_program);
            }
            c.active = d.programs().active(now).map(|p| p.name.clone());
            // The listener's suppression window lives with the other settings
            // and is persisted by the engine `[REQ-VIS-155]`; the Director is
            // told when it moves `[SPEC-PLAY-050]`.
            let want = engine.snapshot_suppress_h();
            if d.suppress_h() != want {
                d.set_suppress_h(want);
            }
        }

        // A passage the engine could not open never played, so the Director
        // must stop counting it as though it had -- otherwise one unreadable
        // file suppresses its recording and its artist for a full rotation
        // `[REQ-PD-112]`.
        for id in engine.take_dropped() {
            if let (Some(note), Some(d)) = (self.notes.remove(&id), self.director.as_mut()) {
                d.forget_queued(note);
            }
        }
        // A note is only useful while its passage can still fail to open, and
        // once admitted it cannot. Pruning to what is still queued bounds the
        // map by the queue depth rather than letting it grow one entry per
        // passage for the life of the process.
        if !self.notes.is_empty() {
            let queued: std::collections::HashSet<i64> =
                engine.queued().map(|e| e.passage_id).collect();
            self.notes.retain(|id, _| queued.contains(id));
        }

        let short = engine.shortfall();
        if short == 0 {
            return;
        }
        let queued: Vec<i64> = engine.queued().map(|e| e.passage_id).collect();
        let mut chosen: Vec<i64> = queued;

        if let Some(d) = &mut self.director {
            for _ in 0..short {
                // The tail is what this passage will follow, so flow is
                // measured from it [SPEC-DIR-160]. On the very first pick of a
                // session there is nothing queued and no flow order.
                let tail = chosen.last().copied();
                let Some(decision) = d.decide(now, &mut self.rng, &chosen, tail) else {
                    // Everything eligible is blocked. Falling back keeps the
                    // radio playing, which [REQ-PD-100] requires; silence would
                    // be a worse answer than a repeat.
                    break;
                };
                let entry = decision.entry;
                if let Some(note) = d.note_queued(entry.passage_id, now) {
                    self.notes.insert(entry.passage_id, note);
                }
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
                let mut entry = entry;
                Self::describe(&self.lib, &mut entry);
                engine.enqueue(entry);
            }
        }

        let still_short = engine.shortfall();
        if still_short > 0 {
            match self.lib.random_radio(still_short) {
                Ok(entries) => entries.into_iter().for_each(|mut e| {
                    Self::describe(&self.lib, &mut e);
                    engine.enqueue(e);
                }),
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

    /// Share programme control with a UI.
    pub fn controls(&self) -> SharedControls {
        Arc::clone(&self.controls)
    }

    /// The programme in force `[SPEC-DIR-180]`.
    pub fn program(&self) -> Option<String> {
        let d = self.director.as_ref()?;
        d.programs().active(unix_now()).map(|p| p.name.clone())
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}
