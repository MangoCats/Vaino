# SONOS011: Closing the Correctness Gaps

**Development Record — built on `Sonos`, 2026-08-28**

[SONOS010](SONOS010-outstanding-implementation-items.md)'s eleven items, nine of them closed in one sequenced pass — the user's own ordering: data model first, then the three correctness fixes, then startup restore, then the settings panel, with the two items that need real hardware or a stopwatch left for the user to run personally.

> **Related:** [SONOS010](SONOS010-outstanding-implementation-items.md) · `player/src/path.rs`, `player/src/sonos.rs`, `player/src/engine.rs`, `player/src/db.rs`, `player/src/web.rs`, `player/src/bin/vaino.rs`, `player/src/web/skins/vaino/skin.{html,js,css}`

---

## 1. What was built

**`[GDE-SONOS-1060]` Exclusivity is now enforced, closing the gap `[GDE-SONOS-1010]` named.** `Command::SetSonosRing`'s own handler in `engine.rs` now calls `self.path.set_playing(ring.is_none() && self.playing)` before setting `self.sonos_ring` — the same silencing mechanism a pause already uses (`[PI3-OPEN-020]`), so the local device stays attached and quiet rather than being torn down, and reopening it on the way back respects whatever `self.playing` already was: a listener who paused before switching to Sonos does not get local output back just because Sonos stopped. Placed in `engine.rs` rather than `sonos.rs` deliberately — only the engine's own command dispatch holds both `self.path` and `self.playing` at once, and giving `sonos.rs` a `PathHandle` of its own would have widened who can touch local output for no reason this fix needed.

**`[GDE-SONOS-1070]` Activation is now rollback-safe, closing `[GDE-SONOS-1020]`.** `sonos::activate()` sends `Command::SetSonosRing(Some(ring))` only *after* `soap::set_uri_and_play` has actually succeeded. A failed SOAP call now returns `Err` with the engine never having been told a ring existed, rather than leaving it believing one was wanted after the encoder that would have fed it had already been dropped.

**`[GDE-SONOS-1080]` A loss-of-control watcher answers `[GDE-SONOS-1030]`, the write-only gap `[SPEC-APS-030]` already existed to prevent.** A background task, spawned per-activation from `web.rs`, re-reads `GetMediaInfo`'s `CurrentURI` every 20 seconds and falls back to local output the moment it no longer matches Vaino's own stream URL — deliberately a plain periodic re-read rather than the fuller backoff state machine `path.rs` runs for local output, since a first version answering "did Music Assistant just take this back" at all was the actual gap, not the retry policy around it. A monotonic `sonos_generation` counter on `Ui`, bumped on every `use`/`forget`, lets a watcher started for an earlier session recognise it has been superseded — checked once before its own network read and once more right after, so a `forget` landing mid-poll still wins over a watcher about to declare a takeover on a session that, by the time it would act, no longer exists.

**`[GDE-SONOS-1090]` Vaino's own volume control now reaches the speaker, closing `[GDE-SONOS-1040]`.** `set_volume` forwards, fire-and-forget on a raw thread never awaited, a `RenderingControl#SetVolume` SOAP call to the active Sonos coordinator whenever one is chosen, mapping Vaino's `-72.0..=0.0` dB range linearly onto Sonos's own `0..=100` `Master` parameter.

**`[GDE-SONOS-1100]` Found in passing, and fixed as directly adjacent cleanup rather than scope creep: two blocking SOAP calls were running straight on a tokio worker thread.** `deactivate()`'s call to `soap::stop` was invoked without `spawn_blocking` at both sites that pre-empt or end a session (`sonos_use`'s replacement of an old session, and `sonos_forget`) — up to five seconds of a tokio worker blocked on an unreachable speaker, a direct violation of the same rule `[SPEC-APS-040]` already states for the engine. Both call sites now wrap the call.

**`[GDE-SONOS-1110]` A unified `SpeakerId` replaces the independent `output_mode`/`sonos_target` pair, closing the design question `[GDE-SONOS-860]` left open.** `path.rs` gains `SpeakerId` (`Local` | `Sonos(SonosTarget)`), `SonosTarget`, and `SonosMember` — plain data, compiled unconditionally rather than behind the `sonos` feature, so a build stores and round-trips a choice regardless of which features it carries. `db.rs`'s `save_chosen_speaker`/`load_chosen_speaker` replace the four functions this superseded, against one `player_settings` key rather than two that could disagree; `SonosTarget.members` carries `#[serde(default)]` so a target persisted before this field existed still loads rather than failing to parse.

**`[GDE-SONOS-1120]` Stereo-pair member detail is now surfaced, closing `[GDE-SONOS-880]`.** `topology::parse_coordinators()` parses Sonos's own `ChannelMapSet` attribute (`RINCON_A:RF,RF;RINCON_B:LF,LF`) to populate `SonosSpeaker.members`/`SonosTarget.members` — every physical unit in a group, coordinator included, each labelled by channel. "Office" is still one row to choose; a listener asking which two speakers that is now gets a real answer.

**`[GDE-SONOS-1130]` Nothing re-activates a remembered Sonos target at startup, closes `[GDE-SONOS-980]` on its own.** `vaino.rs` calls a new `web::spawn_restore_chosen_speaker` right after `Ui` is built, in the background rather than blocking the accept loop: it reads the persisted `SpeakerId`, and if it names a Sonos target, re-resolves it through a fresh `sonos::discover()` by UDN before calling `activate` — never trusting the persisted `last_ip` directly, the same posture `[GDE-SONOS-760]` already established, since the speaker may have moved, been renamed, or simply be off that morning. Local output is never muted unless and until this actually succeeds, so a listener gets sound from *something* either way.

**`[GDE-SONOS-1140]` The settings panel now shows one merged list with one confirm-or-revert flow, closing `[GDE-SONOS-1000]`.** The separate `#sonos` block is gone; Sonos speakers render as rows in the same `<ul id="bt-list">` Bluetooth devices already use, tagged with a `kind` so `act`/`choose`/`revert` dispatch to the right backend without a parallel code path for each. A Sonos row gets the identical 30-second "can you hear it?" safety net `[PI3-UI-030]` already protects a Bluetooth choice with, rather than the simpler, confirm-free flow `[GDE-SONOS-930]` shipped first — a wrong choice can strand a listener with no way to hear whether it worked whichever kind of speaker it named. A merged row also carries stereo-pair member detail (`[GDE-SONOS-1120]`, above) when there is more than one unit to name, and the local-device row now reads "Connected, but silent" rather than "Playing here" whenever Sonos holds the floor, since `path.set_playing(false)` (`[GDE-SONOS-1060]`) leaves that device's sink name in place even though it is not what is audible.

**`[GDE-SONOS-1150]` Switching to Sonos mid-crossfade disturbs nothing already sounding, verified rather than assumed, closing `[GDE-SONOS-870]`.** A new engine test builds two overlapping passages, ticks until both are live, sends `SetSonosRing(Some(...))` mid-fade, and asserts `self.live` is untouched — because the handler touches only `self.path` and `self.sonos_ring`, exactly as `ReopenOutput` and a Bluetooth reconnect already do, there was never a mechanism by which it *could* restart, skip, or reorder a fade in flight. Bluetooth's own behaviour was never itself under test either; this closes both at once by testing the shared mechanism they both rely on.

---

## 2. Measured, not assumed

**`[GDE-SONOS-1160]` The full verification sweep, run after every logical step and once more at the end:** `cargo build`/`cargo test --lib`/`cargo clippy --all-targets`, each run both with `--features sonos` and without. 333 lib tests pass with the feature enabled (up from 328 at the close of [SONOS009](SONOS009-implementation-record.md)), 325 without; clippy is clean on both aside from one pre-existing, unrelated `dead_code` warning in `tests/invariants.rs` present regardless of this work. `node build/verify-skins.js` renders every skin clean, `vaino` included, against the merged settings-panel markup.

---

## 3. Left open, on purpose

**Never run end-to-end against the real Office pair, still true as `[GDE-SONOS-990]` already said, and still deliberately so.** Every write path built or changed in this pass — activation, deactivation, the loss-of-control watcher's own read, volume forwarding — has been exercised against the SOAP fixtures and unit tests only. The user has said this will be tested personally, against the physical household pair, rather than attempted here.

**Encoder latency remains unmeasured, per `[GDE-SONOS-290]`/`[GDE-SONOS-400]`.** Nothing in this pass touched the encoder path; the user has said this will be measured personally, with a stopwatch against the real speaker, rather than attempted here.

---

**Traceability:** `[GDE-SONOS-1060..1160]` · closes `[GDE-SONOS-1010]`, `[GDE-SONOS-1020]`, `[GDE-SONOS-1030]`, `[GDE-SONOS-1040]`, `[GDE-SONOS-860]`, `[GDE-SONOS-870]`, `[GDE-SONOS-880]`, `[GDE-SONOS-980]`, `[GDE-SONOS-1000]` · leaves `[GDE-SONOS-990]`, `[GDE-SONOS-290]`, `[GDE-SONOS-400]` open by the user's own choice
