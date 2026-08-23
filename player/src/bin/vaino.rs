//! Vaino: continuous radio with a web UI.
//!
//! Two threads by necessity, not by preference. `cpal`'s stream is not `Send`
//! on every backend, so the engine must be built and pumped on one thread that
//! owns it; the web server runs on tokio and touches playback only through
//! [`EngineHandle`], which is the whole control surface `[REQ-VIS-140]`.
//!
//! Usage:  vaino <vaino.db> [--port N] [--depth N] [--device NAME]
//!
//! `--device` takes a case-insensitive substring of the output device name.
//! It matters more than it looks: PipeWire offers a `Dummy Output` whenever no
//! real sink is present, and a Bluetooth speaker that is momentarily absent at
//! startup leaves the player attached to that dummy -- playing perfectly into
//! nothing, and reporting itself healthy `[IMPL-AUD-010]`.

use std::path::PathBuf;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::Duration;

use vaino_player::engine::Engine;
use vaino_player::session::{Explanations, Session, SharedControls};
use vaino_player::web;
use vaino_player::BUFFER_FRAMES;

/// How far ahead of the outgoing side the incoming one is told to start
/// `[SPEC-BK-065]`.
///
/// It begins in a moment rather than now, so handing it the position as of
/// *now* would replay that moment. MPD was measured at 14-27 ms from the
/// command to its first frame `[SPEC-BK-055]`; the rest of this is the fade
/// the outgoing side is still running through. Small enough that being wrong
/// by all of it is a fraction of a second of music, in one direction or the
/// other, once.
const HANDOFF_LEAD_MS: u64 = 250;


/// A string-valued flag, absent when not given.
fn text_flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn flag(args: &[String], name: &str, default: usize) -> usize {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("vaino {}", vaino_player::build_id());
        eprintln!("usage: vaino <vaino.db> [--port N] [--depth N] [--device NAME]");
        eprintln!("       [--mpd HOST:PORT --mpd-root MUSIC_DIRECTORY]");
        std::process::exit(2);
    }
    if args.iter().any(|a| a == "--version") {
        println!("vaino {}", vaino_player::build_id());
        return;
    }
    println!("vaino {}", vaino_player::build_id());
    let db = PathBuf::from(&args[0]);
    let port = flag(&args, "--port", 5720);
    let depth = flag(&args, "--depth", 5);
    let device = text_flag(&args, "--device");
    // A guest backend, offered rather than assumed `[SPEC-BK-020]`. Vaino still
    // plays; MPD is attached and idle until a switch asks for it.
    let mpd_addr = text_flag(&args, "--mpd");
    let mpd_root = text_flag(&args, "--mpd-root");

    // The listening -- plays, preferences, programmes -- is the one thing in
    // that file nothing can rebuild `[REQ-LIB-160]`. One snapshot now, so a
    // library that has never been backed up stops being so within a second of
    // starting, then hourly for as long as it runs.
    {
        let backup_db = db.clone();
        std::thread::Builder::new()
            .name("vaino-backup".into())
            .spawn(move || loop {
                match vaino_player::backup::snapshot(&backup_db) {
                    Ok(p) => println!("listener state backed up to {}", p.display()),
                    // Never fatal: a player that stops playing because it could
                    // not write a backup has turned a precaution into the fault.
                    Err(e) => eprintln!("listener backup failed ({e}); playback continues"),
                }
                std::thread::sleep(Duration::from_secs(3600));
            })
            .ok();
    }

    // Album names and the browse index come from the files' own tags, and
    // reading them takes ~18 s for five thousand files. Doing it here, in the
    // background, is the difference between a feature that works and one that
    // waits for someone to remember a command -- which is exactly how the
    // browse pages came up empty in the first place. Incremental, so it is a
    // no-op on every start after the first, and off the audio path entirely.
    {
        let scan_db = db.clone();
        std::thread::Builder::new()
            .name("vaino-tagscan".into())
            .spawn(move || {
                if let Err(e) = vaino_player::tags::backfill(&scan_db, true) {
                    eprintln!("tag scan unavailable ({e}); album names will be missing");
                }
            })
            .ok();
    }

    // The web side needs the library too, to read cover art out of the files
    // [REQ-VIS-170]; the engine thread takes ownership of the path itself.
    let art_db = db.clone();

    // The engine thread builds everything it owns, then reports its handle
    // back. Nothing audio-related crosses a thread boundary afterwards.
    let (tx, rx) = sync_channel(1);
    std::thread::Builder::new()
        .name("vaino-engine".into())
        .spawn(move || engine_thread(db, depth, device, mpd_addr, mpd_root, tx))
        .expect("spawn engine thread");

    let (handle, why, controls) = match rx.recv() {
        Ok((h, why, c)) => (Arc::new(h), why, c),
        Err(_) => {
            eprintln!("engine failed to start");
            std::process::exit(1);
        }
    };
    let ui = web::Ui { handle, why, controls, db: art_db };

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port as u16));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cannot listen on {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("web UI on http://localhost:{port}/");
    if let Err(e) = axum::serve(listener, web::router(ui)).await {
        eprintln!("server: {e}");
    }
}

/// The published state, so the loop can read the listener's settings after the
/// engine has become an anonymous backend.
fn state_of(
    h: &vaino_player::engine::EngineHandle,
) -> std::sync::Arc<std::sync::Mutex<vaino_player::engine::PlayerState>> {
    h.state.clone()
}

fn engine_thread(
    db: PathBuf,
    depth: usize,
    device: Option<String>,
    mpd_addr: Option<String>,
    mpd_root: Option<String>,
    tx: std::sync::mpsc::SyncSender<(
        vaino_player::engine::EngineHandle,
        Explanations,
        SharedControls,
    )>,
) {
    let mut session = match Session::open(&db, depth) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    println!("library: {} radio passages", session.lib.count_radio().unwrap_or(0));

    // A missing device must not stop the process: the UI still needs to come
    // up and say so, which is more use than exiting silently.
    // The supervisor opens the device on its own thread and owns it from
    // there `[SPEC-APS-060]`; this reports only what happened.
    let (path, why) = vaino_player::path::start(device, BUFFER_FRAMES * 2);
    println!("{why}");

    let (mut engine, handle) = Engine::new(path, session.depth());
    // Taken before the handle is sent away: the loop below reads the listener's
    // settings from here once the engine has become an anonymous backend.
    let published = state_of(&handle);
    session.prime(&mut engine);
    if tx.send((handle, session.explanations(), session.controls())).is_err() {
        return; // nobody left to control it
    }

    // Resume the play STATE, not just the position `[PI5-PWR-030]`.
    //
    // `player_state` has always recorded whether it was playing, and the
    // session read that column and threw it away -- so an appliance that lost
    // power, or was shut down deliberately from the settings page, came back
    // holding its place and silent. Restoring the position but not the
    // intention is half a resume.
    //
    // Safe against a missing speaker without waiting for one. Playing marks
    // the supervisor's interest, it checks for a dummy sink immediately rather
    // than up to WATCH later, and a dummy is treated as a failure -- which
    // makes `path.audible()` false, and the engine advances nothing while
    // nobody can hear it. So this resumes into silence only for as long as it
    // takes to notice, and the position is not spent.
    if session.resume_playing() {
        println!("resuming playback: it was playing when it last stopped");
        engine.play_on_resume();
    }

    // From here the engine is a **backend** rather than the engine
    // `[SPEC-BK-020]`. Everything above is setup, which is the process's
    // business and not a backend's -- priming, the store, the resume.
    //
    // The suppression windows are read from the published state rather than
    // from the engine, because the engine is about to stop being reachable by
    // name. They are the listener's settings and the same whoever plays.
    let saved_cue = vaino_player::db::PlayerStore::open(&db)
        .ok()
        .and_then(|s| s.load_settings())
        .map(|s| s.cue_sheets)
        .unwrap_or(false);
    // Read regardless of the feature so the flags parse and report the same
    // either way; only the guest that consumes them is gated.
    #[cfg(not(feature = "mpd"))]
    let _ = (&mpd_addr, &mpd_root, &saved_cue);
    let controls_for_switch = session.controls();
    if let Ok(mut c) = controls_for_switch.lock() {
        c.backend = Some("vaino".into());
    }
    let mut backend = vaino_player::switch::Switching::new(Box::new(engine));

    #[cfg(feature = "mpd")]
    if let Some(addr) = mpd_addr {
        let root = mpd_root.unwrap_or_default();
        match vaino_player::mpd_backend::MpdBackend::connect(
            &addr,
            &root,
            session.depth(),
            vaino_player::SAMPLE_INTERVAL_MS,
        ) {
            Ok(mut guest) => {
                if let Ok(c) = rusqlite::Connection::open(&db) {
                    match vaino_player::mpd_backend::nameable_uris(&c, &root) {
                        Ok(n) => guest.attach_names(n),
                        Err(e) => eprintln!("cannot name URIs for MPD ({e})"),
                    }
                    // Cue tracks, only when the listener has asked for them
                    // `[REQ-VIS-205]`. Read at startup: sheets written now are
                    // used from the next run, which the settings page says.
                    if saved_cue {
                        match vaino_player::mpd_backend::cue_uris(&c, &root) {
                            Ok(m) => {
                                println!("{} passage(s) have a cue track to be named by", m.len());
                                guest.attach_cues(m);
                            }
                            Err(e) => eprintln!("cannot map cue tracks ({e})"),
                        }
                    }
                }
                if let Ok(st) = vaino_player::db::PlayerStore::open(&db) {
                    guest.attach_store(st);
                }
                backend.attach_guest(Box::new(guest));
                println!("MPD guest attached at {addr}; Vaino is still the one playing");
                if let Ok(mut c) = controls_for_switch.lock() {
                    c.guest_available = true;
                    c.guest_name = Some(format!("MPD at {addr}"));
                    // Something true before the first switch, so the control is
                    // evidently working rather than evidently blank.
                    c.switch_status = Some("Vaino is playing; the guest is attached and idle".into());
                }
            }
            Err(e) => eprintln!("MPD guest unavailable ({e}); continuing on the local engine"),
        }
    }

    // Otherwise paused until told otherwise. The producers fill regardless, so
    // pressing Play in the browser starts on a primed pipeline rather than an
    // underrun [REQ-AUD-142].
    while !vaino_player::playback::Playback::is_shutdown(&backend) {
        let submitted = vaino_player::playback::Playback::tick(&mut backend);
        // Continuous radio: the queue never runs dry, so playback never ends.
        // The backend plays; the settings belong to the process `[SPEC-BK-020]`.
        let suppress = published
            .lock()
            .map(|s| (s.skip_suppress_h, s.dequeue_suppress_h))
            .unwrap_or((vaino_player::SKIP_SUPPRESS_H, vaino_player::DEQUEUE_SUPPRESS_H));
        // The four folder-writing settings, run here rather than in a request
        // handler: each walks the library and writes into a folder Vaino does
        // not own `[REQ-VIS-205]`, which is not work to do while a browser
        // waits.
        //
        // **These four and the `writes_files!` arms in `web.rs` are one list in
        // two places.** A setting with a route and no entry here is a checkbox
        // that persists and does nothing; one with an entry and no route cannot
        // be asked for. Change either and change both.
        run_generation(&db, &controls_for_switch, "cue sheets", "sheets",
            |c| c.cue_requested.take(), |c, s| c.cue_status = Some(s),
            |conn| vaino_player::cue::generate(conn, false).map(|r| (r, "cue sheet")));
        run_generation(&db, &controls_for_switch, "cover art", "covers",
            |c| c.covers_requested.take(), |c, s| c.covers_status = Some(s),
            |conn| vaino_player::covers::generate(conn, false).map(|r| (r, "cover")));
        run_generation(&db, &controls_for_switch, "lyrics sidecar", "files",
            |c| c.sidecar_requested.take(), |c, s| c.sidecar_status = Some(s),
            |conn| vaino_player::lyrics_sidecar::generate(conn, false).map(|r| (r, "file")));
        // The odd one out: it writes into a client's cache rather than the
        // music folder, so it has somewhere to fail to find.
        run_generation(&db, &controls_for_switch, "lyrics cache", "files",
            |c| c.lyrics_requested.take(), |c, s| c.lyrics_status = Some(s),
            |conn| match vaino_player::lyrics_cache::cache_dir() {
                // Not a failure: a machine the client has never run on has
                // nothing useful to write there `[SPEC-LYR-075]`.
                None => Err("no client cache on this machine; nothing written".to_string()),
                Some(dir) => vaino_player::lyrics_cache::generate(conn, &dir, false)
                    .map(|r| (r, "song")),
            });
        // A switch asked for by the browser happens here, where the backends
        // are `[SPEC-BK-030]`. Taken before the refill so the incoming side is
        // topped up rather than the outgoing one.
        let asked = controls_for_switch.lock().ok().and_then(|mut c| c.switch_requested.take());
        if let Some(which) = asked {
            let target = if which == "mpd" {
                vaino_player::switch::Side::Guest
            } else {
                vaino_player::switch::Side::Local
            };
            // Seamless: the passage that is playing crosses at the position it
            // has reached, and the outgoing side is not silenced until the
            // incoming one is audible `[SPEC-BK-065]`.
            let said = match session.hand_over_seamless(&mut backend, target, 600, HANDOFF_LEAD_MS)
            {
                Ok(h) => {
                    let how = match h.stopped {
                        Some(vaino_player::switch::Stopped::Faded) => "faded",
                        Some(vaino_player::switch::Stopped::Cut) => "cut",
                        None => "already there",
                    };
                    let lost = if h.carried.lost.is_empty() {
                        String::new()
                    } else {
                        format!(", {} could not be carried", h.carried.lost.len())
                    };
                    // Said rather than assumed: a handoff that found nothing
                    // playing, or one the other side never answered, is a
                    // different event from a seamless one `[PI3-API-030]`.
                    let seam = match (h.resumed, h.took_ms) {
                        (Some((_, at)), Some(ms)) => {
                            format!(", resumed {:.1}s in after {ms} ms", at as f64 / 1000.0)
                        }
                        (Some((_, at)), None) => format!(
                            ", resumed {:.1}s in but the other side never sounded",
                            at as f64 / 1000.0
                        ),
                        (None, _) => ", nothing was playing to carry".to_string(),
                    };
                    format!(
                        "now on {which} ({how}, {} passage(s) carried{lost}{seam})",
                        h.carried.moved.len()
                    )
                }
                Err(e) => format!("refused: {e}"),
            };
            println!("switch: {said}");
            if let Ok(mut c) = controls_for_switch.lock() {
                c.switch_status = Some(said);
                c.backend = Some(which);
            }
        }

        // A seek asked for by the browser, applied to the side that is
        // sounding `[REQ-VIS-225]`. Taken here for the same reason a switch is:
        // the backends live on this thread and nowhere else.
        let seek_ask = controls_for_switch.lock().ok().and_then(|mut c| c.seek_requested.take());
        if let Some(ms) = seek_ask {
            vaino_player::playback::Playback::seek_to(&mut backend, ms);
        }

        // What the side now sounding can do, published where the browser can
        // read it `[SPEC-BK-040]`. Taken every pass rather than at a switch,
        // so it is right even for the side the player started on.
        {
            let seekable = vaino_player::playback::Playback::capabilities(&backend).seek;
            if let Ok(mut c) = controls_for_switch.lock() {
                if c.can_seek != seekable {
                    c.can_seek = seekable;
                }
            }
        }
        session.refill(&mut backend, suppress);
        // Nothing submitted means the ring is comfortably full -- the engine
        // declines to mix less than a threshold's worth -- so there is time to
        // spare. Sleeping through it is the difference between a loop that
        // wakes hundreds of times a second to move a handful of samples and one
        // that wakes when there is work. The ring holds ~14 s, so this pause is
        // three orders of magnitude inside the margin.
        if submitted == 0 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Run one folder-writing setting when the browser has asked for it.
///
/// The four of them `[REQ-VIS-205]`, `[REQ-VIS-210]`, `[REQ-VIS-215]`,
/// `[REQ-VIS-220]` differ only in which intent cell they read, which module
/// they call and what they call the files. Written four times they drifted --
/// one of them was pasted with the wrong status field and reported into another
/// setting's line -- so they are written once.
///
/// **Turning one off leaves what was written.** Deleting files from someone's
/// music folder is a larger act than declining to add more, and is not what
/// unticking a box asked for; `off_noun` names what stays.
fn run_generation(
    db: &std::path::Path,
    controls: &SharedControls,
    label: &str,
    off_noun: &str,
    take: impl Fn(&mut vaino_player::session::Controls) -> Option<bool>,
    status: impl Fn(&mut vaino_player::session::Controls, String),
    run: impl Fn(
        &rusqlite::Connection,
    ) -> Result<(vaino_player::report::Written, &'static str), String>,
) {
    let Some(asked) = controls.lock().ok().and_then(|mut c| take(&mut c)) else { return };
    let said = if !asked {
        format!("off; {off_noun} already written are left alone")
    } else {
        match rusqlite::Connection::open(db).map_err(|e| e.to_string()).and_then(|c| run(&c)) {
            Ok((rep, noun)) => {
                for f in &rep.failed {
                    eprintln!("{label}: {f}");
                }
                rep.summary(noun)
            }
            Err(e) => e,
        }
    };
    println!("{label}: {said}");
    if let Ok(mut c) = controls.lock() {
        status(&mut c, said);
    }
}
