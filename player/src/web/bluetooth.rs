//! Bluetooth speaker discovery and pairing, reached from the audio-source
//! panel `[PI3-AIM-020]`.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::bluetooth;
use crate::engine::Command;

use super::Ui;

/// Every radio and whether it is blocked `[PI3-RF-010]`.
///
/// Built because a blocked radio is indistinguishable, from the settings page,
/// from a broken button: the Middleton was paired, bonded, trusted and
/// advertising, and Connect did nothing at all because `hci0` was soft-blocked
/// `[PI3-FOUND-050]`. One line saying so would have ended that evening.
pub(super) async fn radios() -> Response {
    bt_reply(bluetooth::run(bluetooth::Verb::Radios, None), false)
}

/// Switch one radio on or off `[PI3-RF-020]`.
///
/// The helper refuses to block whichever radio carries the default route, and
/// this deliberately does **not** repeat that rule -- one copy, on the side
/// that holds the privilege, so a second caller cannot be told something
/// different `[PI3-RF-030]`.
pub(super) async fn set_radio(
    axum::extract::Path((kind, state)): axum::extract::Path<(String, String)>,
) -> Response {
    let on = match state.as_str() {
        "on" => true,
        "off" => false,
        _ => return (StatusCode::NOT_FOUND, "state is on or off").into_response(),
    };
    bt_reply(bluetooth::set_radio(&kind, on), false)
}

pub(super) async fn speakers() -> Response {
    bt_reply(bluetooth::run(bluetooth::Verb::List, None), false)
}

/// Verbs that name no device: `scan`.
pub(super) async fn speaker_verb(
    State(ui): State<Ui>,
    axum::extract::Path(verb): axum::extract::Path<String>,
) -> Response {
    let Some(v) = bluetooth::Verb::parse(&verb) else {
        return (StatusCode::NOT_FOUND, "unknown verb").into_response();
    };
    if v.needs_address() {
        return (StatusCode::BAD_REQUEST, "verb needs a device").into_response();
    }
    let _ = &ui;
    bt_reply(bluetooth::run(v, None), false)
}

/// Verbs that name a device. `use` reopens the player output as part of the
/// same request `[PI3-UI-020]`: the stream does not dependably follow a change
/// of default sink, so a selection that stopped at the helper would look like
/// it worked and be silent. Doing it here rather than in the browser means no
/// caller can forget the step that makes the choice audible.
pub(super) async fn speaker_verb_on(
    State(ui): State<Ui>,
    axum::extract::Path((verb, address)): axum::extract::Path<(String, String)>,
) -> Response {
    let Some(v) = bluetooth::Verb::parse(&verb) else {
        return (StatusCode::NOT_FOUND, "unknown verb").into_response();
    };
    let result = bluetooth::run(v, Some(&address));
    let reopen = result.is_ok() && matches!(v, bluetooth::Verb::Use | bluetooth::Verb::Pair);
    if reopen {
        // Remembered so the appliance's own reconnect timer knows which
        // absent device is worth paging `[PI3-AIM-020]`, `[REQ-VIS-260]` --
        // best-effort, since a listener whose speaker just started working
        // should not be told it failed over a bookkeeping write.
        let db = ui.db.clone();
        let _ = tokio::task::spawn_blocking(move || match crate::db::PlayerStore::open(&db) {
            Ok(store) => {
                if let Err(e) = store.save_speaker_address(&address) {
                    eprintln!("save speaker address: {e}");
                }
            }
            Err(e) => eprintln!("save speaker address: {e}"),
        })
        .await;
        ui.handle.send(Command::ReopenOutput);
        // Wait for the reopen to land before reporting where the audio went.
        // Reading the sink immediately gives sink:null with dummy:false, which
        // reads as healthy and is merely early -- the precise shape of
        // reassuring answer that hid this fault in the first place.
        tokio::time::sleep(Duration::from_millis(1_500)).await;
    }
    bt_reply(result, reopen)
}

/// One shape for every speaker reply, so the panel has one thing to read.
pub(super) fn bt_reply(result: Result<serde_json::Value, String>, reopened: bool) -> Response {
    match result {
        Ok(mut v) => {
            if let Some(obj) = v.as_object_mut() {
                obj.insert("reopened".into(), reopened.into());
                // The answer the listener actually cares about, and the one
                // the helper cannot give: where the audio ended up
                // `[PI3-API-020]`.
                let where_to = crate::sink::current();
                // An unlinked stream is not a working one. Saying so keeps
                // "we could not tell" distinct from "it is fine".
                if where_to.known && where_to.sink.is_none() {
                    obj.insert("audible".into(), serde_json::Value::Null);
                } else {
                    obj.insert("audible".into(), (!where_to.dummy).into());
                }
                if let Ok(s) = serde_json::to_value(where_to) {
                    obj.insert("output".into(), s);
                }
            }
            axum::Json(v).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

