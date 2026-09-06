//! Launching Sampo's own console from Vaino's browse page `[REQ-VIS-320]` --
//! the mirror image of `tools/vaino_control.py`'s `ensure_vaino()`: that
//! module finds and starts a co-resident Vaino on demand, from Sampo's own
//! browser; this finds and starts a co-resident Sampo the same way, from
//! Vaino's own browse page, for the one link a desktop listener curating
//! their own library might actually want to follow without opening a
//! terminal.
//!
//! Gated behind `sampo-support` exactly like `/review` and `/edit`
//! (`mod.rs`) -- Sampo is x86-desktop-only (`README.md`), and a build that
//! never runs Sampo has nothing to offer here. Not even a route exists on
//! a build without the feature, the same posture the other two take.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::{IntoResponse, Response};

use super::Ui;

/// `tools/console.py`'s own default, mirrored rather than re-derived --
/// the two processes must agree on where to look for each other without
/// either configuring the other.
const SAMPO_PORT: u16 = 5730;

/// A socket question, not a route question -- the same shape
/// `vaino_control.py::_vaino_reachable` asks in reverse.
fn sampo_reachable() -> bool {
    format!("127.0.0.1:{SAMPO_PORT}")
        .parse()
        .ok()
        .map(|addr| std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok())
        .unwrap_or(false)
}

/// Where `tools/console.py` is, if it can be found at all `[REQ-VIS-320]`.
///
/// Checked against this repository's own build layout, relative to
/// *Vaino's own running binary* -- `player/target/release/vaino[.exe]`,
/// three directories below the repository root -- the same "developed
/// side by side" case `vaino_control.py::_vaino_binary` already checks in
/// reverse. Never guessed beyond that, and never a `PATH` search: unlike a
/// `vaino` binary, a Python script has no reason to be installed anywhere
/// generic, and a wrong script started against the wrong database is
/// worse than admitting there is none.
fn console_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let candidate = exe.parent()?.parent()?.parent()?.parent()?.join("tools").join("console.py");
    candidate.is_file().then_some(candidate)
}

/// Whichever Python actually runs on this machine, checked rather than
/// assumed -- `python3` first (the name every non-Windows install uses),
/// falling back to plain `python` (Windows' own convention).
fn python() -> Option<&'static str> {
    ["python3", "python"].into_iter().find(|&candidate| {
        Command::new(candidate)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// Whether the browse page's Sampo link is worth showing at all
/// `[REQ-VIS-320]` -- probed once when the page loads, the same shape
/// `vaino_control.py::_vaino_has_sampo_support` checks in reverse.
/// Already running counts, so a listener who started Sampo by hand still
/// sees the link; otherwise it takes both a locatable console and a
/// Python to run it with, since offering a link neither `sampo_ensure`
/// verb could ever satisfy would be worse than no link at all.
pub(super) async fn sampo_available() -> Response {
    let available = tokio::task::spawn_blocking(|| {
        sampo_reachable() || (console_script().is_some() && python().is_some())
    })
    .await
    .unwrap_or(false);
    axum::Json(serde_json::json!({ "available": available })).into_response()
}

/// Start the co-resident Sampo console if one is not already there
/// `[REQ-VIS-320]`, on *this* Vaino's own database path -- the same reason
/// `ensure_vaino` starts a fresh Vaino on Sampo's own database path in
/// reverse: the link should open the same library this browse page is
/// already showing, not a different one. Launched without `--root`
/// (Vaino has no single "music folder" to offer -- only individual
/// passage paths); Sampo's own console already treats that as a normal,
/// supported case, with only its Folder view left empty (`HOWTO.md §5`).
pub(super) async fn sampo_ensure(State(ui): State<Ui>) -> Response {
    if sampo_reachable() {
        return axum::Json(serde_json::json!({ "ok": true, "port": SAMPO_PORT })).into_response();
    }
    let db = ui.db.clone();
    let started = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let script = console_script()
            .ok_or_else(|| "no local Sampo console found (tools/console.py)".to_string())?;
        let py = python().ok_or_else(|| "no python3/python found on PATH".to_string())?;
        Command::new(py)
            .arg(&script)
            .arg(&db)
            .arg("--port")
            .arg(SAMPO_PORT.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not start sampo: {e}"))
    })
    .await;
    if let Err(why) = started.unwrap_or_else(|_| Err("could not start sampo".into())) {
        return axum::Json(serde_json::json!({ "ok": false, "error": why })).into_response();
    }
    // Polled, not a fixed wait -- the same reasoning `ensure_vaino` gives
    // for not guessing a sleep duration against a console whose own
    // startup time scales with library size.
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if sampo_reachable() {
            return axum::Json(serde_json::json!({ "ok": true, "port": SAMPO_PORT })).into_response();
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    axum::Json(serde_json::json!({
        "ok": false,
        "error": "sampo did not answer within 20s of starting"
    }))
    .into_response()
}
