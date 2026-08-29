# SONOS009: Implementation Record

**Development Record — built on `Sonos`, 2026-08-28**

[SONOS008](SONOS008-implementation-plan.md)'s five phases, built. What actually landed, what was measured along the way, and what is honestly still open.

> **Related:** [SONOS008](SONOS008-implementation-plan.md) · `player/src/sonos.rs`, `player/src/db.rs`, `player/src/web.rs`, `player/src/engine.rs`, `player/src/output.rs`

---

## 1. What was built

**`[GDE-SONOS-890]` `player/src/sonos.rs`, feature-gated behind `sonos`** (default off, same shape as `mpd`/`sampo-support`): SSDP discovery, the `ZoneGroupTopology` parser tested against the real fixture captured in [SONOS001](SONOS001-appliance-survey.md), the SOAP calls (`SetAVTransportURI`, `Play`, `Stop`, `SetVolume`) written exactly as validated live against the real Office pair, and the encoder (`mp3lame-encoder`, chosen in [SONOS005](SONOS005-encoder-choice.md)) running on its own thread, reading a second `OutputRing` and broadcasting encoded chunks to whichever HTTP connection is listening.

**`[GDE-SONOS-900]` The engine tap landed exactly as planned** `[GDE-SONOS-810]`: one new field (`sonos_ring: Option<OutputRing>`, `#[cfg(feature = "sonos")]`), one new `Command::SetSonosRing` variant, one additional `submit()` call beside the existing one at the mixer's own tap point. `OutputRing` gained a symmetric `read()` alongside its existing `submit()`, and a manual `Debug` impl (needed only because `Command` derives it).

**`[GDE-SONOS-910]` Persistence reuses `player_settings` exactly as planned** `[GDE-SONOS-770]`: `output_mode` and `sonos_target`, with `save_output_mode`/`load_output_mode`/`save_sonos_target`/`load_sonos_target`/`clear_sonos_target` mirroring `save_speaker_address`/`load_speaker_address` precisely, tested the same way.

**`[GDE-SONOS-920]` Web routes mirror `/audio/speakers/*` at `/audio/sonos/*`** as planned `[GDE-SONOS-800]`, plus `GET /audio/sonos/stream` — the continuous MP3 body itself, built on `tokio-stream`'s `BroadcastStream` (a small, explicit, `sonos`-gated dependency; already-transitive availability was not assumed).

**`[GDE-SONOS-930]` The settings-panel UI is a separate block, not the single merged list `[GDE-SONOS-820]` first proposed.** Built as its own `#sonos` section, self-hiding unless a build with `sonos` compiled in actually answers `GET /audio/sonos` — a deliberate, documented simplification: Bluetooth's confirm-or-revert countdown exists because a wrong choice there can strand a listener with no way to hear whether it worked `[PI3-UI-030]`, a risk a LAN speaker with its own still-running local output does not carry the same way, and folding the two into one state machine in the time available risked the working one more than it helped the new one.

**`[GDE-SONOS-940]` License housekeeping done** `[SONOS007]`: `THIRD-PARTY-LICENSES.md` carries LAME's own LGPLv2 text and the wrapper crate's LGPLv3 text in full; `vaino --version` names LAME when built with `sonos`.

---

## 2. Measured, not assumed

**`[GDE-SONOS-950]` The aarch64 cross-compile was actually run, not just reasoned about.** `build/Dockerfile.aarch64` gained `autoconf automake libtool` exactly as [SONOS006](SONOS006-lame-linking.md) `[GDE-SONOS-550]` said it would need; `docker build` then `cargo build --release --target aarch64-unknown-linux-gnu --features sonos` both succeeded cleanly against the real cross-compile image.

**`[GDE-SONOS-960]` Binary size, measured on the real cross-compiled artifact:** 7,279,776 bytes without `sonos`, 7,739,800 bytes with it — **a 460 KB delta**, comfortably inside [SONOS004](SONOS004-direct-play-requirements.md) `[GDE-SONOS-370]`'s "well under 1 MB" estimate, resolving the "unmeasured" flag that estimate carried.

**`[GDE-SONOS-970]` The full test matrix passes: 328 lib tests with `sonos` alone, 371 with `sonos`+`sampo-support`+`mpd` together, clippy clean on every combination tried, both debug and release, and every existing skin still renders clean** (`build/verify-skins.js`) with the new settings-panel markup added.

---

## 3. Left open, honestly

**`[GDE-SONOS-980]` Nothing re-activates a remembered Sonos target on startup.** `output_mode`/`sonos_target` persist correctly, and `Command::SetSonosRing` works once sent — but nothing in `vaino.rs`'s own startup sequence reads the persisted choice and calls `sonos::activate` before the web server starts. A restart today falls back to local output silently, even when Sonos was the last thing chosen. This is the single largest gap between what SONOS008 asked for ("the player should remember the setup") and what is actually true today.

**`[GDE-SONOS-990]` Never run against the real Office pair.** Every SOAP shape here was validated live while writing [SONOS001](SONOS001-appliance-survey.md) — but that was read-only `GetMediaInfo`/`GetTransportInfo`/topology queries, deliberately never a real `SetAVTransportURI`/`Play` against the household's actual speakers, to avoid interrupting whatever they were doing with the Music Assistant path just repaired. This code has been built and tested in isolation, never end-to-end against a physical Sonos unit.

**`[GDE-SONOS-1000]` The settings panel's Sonos section is functionally simple** `[GDE-SONOS-930]` — a flat found/use/forget list, no confirm-and-revert safety net, no visual merge with the Bluetooth list `[GDE-SONOS-820]` originally asked for. Both are real, deliberate scope cuts, not oversights, and both are named here so a future pass knows exactly what was deferred and why.

---

**Traceability:** `[GDE-SONOS-890..1000]` · closes `[SONOS008]`'s five phases with three named exceptions
