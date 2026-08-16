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
use vaino_player::output::Output;
use vaino_player::session::{Explanations, Session, SharedControls};
use vaino_player::web;
use vaino_player::BUFFER_FRAMES;

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
        eprintln!("usage: vaino <vaino.db> [--port N] [--depth N] [--device NAME]");
        std::process::exit(2);
    }
    let db = PathBuf::from(&args[0]);
    let port = flag(&args, "--port", 5720);
    let depth = flag(&args, "--depth", 5);
    let device = text_flag(&args, "--device");

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
        .spawn(move || engine_thread(db, depth, device, tx))
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

fn engine_thread(
    db: PathBuf,
    depth: usize,
    device: Option<String>,
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
    let out = match Output::open_device(device.as_deref(), BUFFER_FRAMES * 2) {
        Ok(o) => {
            println!("output: {} @ {} Hz, {} ch", o.device_name, o.sample_rate, o.channels);
            Some(o)
        }
        Err(e) => {
            eprintln!("no audio device ({e}); running without output");
            None
        }
    };

    let (mut engine, handle) = Engine::new(out, session.depth());
    session.prime(&mut engine);
    if tx.send((handle, session.explanations(), session.controls())).is_err() {
        return; // nobody left to control it
    }

    // Paused until told otherwise. The producers fill regardless, so pressing
    // Play in the browser starts on a primed pipeline rather than an underrun
    // [REQ-AUD-142].
    while !engine.is_shutdown() {
        let submitted = engine.tick();
        // Continuous radio: the queue never runs dry, so playback never ends.
        session.refill(&mut engine);
        if submitted == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
