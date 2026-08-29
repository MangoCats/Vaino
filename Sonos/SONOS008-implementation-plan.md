# SONOS008: Implementation Plan — Sonos Output, on vainopi and on Windows

**Development Guidance — planned on `Sonos`, 2026-08-28**

Everything [SONOS001](SONOS001-appliance-survey.md)–[SONOS007](SONOS007-lgpl-implications.md) established, turned into a build order: detect and select a Sonos target from Vaino's own UI, remember the choice, present it beside the Bluetooth speakers that already have exactly this UI today, and never run two outputs at once. Grounded in the actual code — `player/src/bluetooth.rs`, `player/src/web.rs`'s `/audio/speakers/*` routes, `player_settings` — not a fresh design invented beside them.

> **Related:** [SONOS001–007](SONOS001-appliance-survey.md) · [SPEC011](../docs/spec/SPEC011-audio-path-supervisor.md) — the aspirational `PathState`/`SpeakerId` design this partially realizes · `player/src/bluetooth.rs`, `player/src/web.rs` (`/audio/speakers/*`), `player/src/db.rs` (`save_speaker_address`) — the pattern this mirrors

---

## 1. What already exists, and what this is honestly extending

**`[GDE-SONOS-740]` Bluetooth output selection is already a solved, shipped feature — this is not designing from nothing.** `bluetooth::Verb` (`List`, `Scan`, `Pair`, `Repair`, `Use`, `Forget`, `Status`, `Radios`), routed at `GET /audio/speakers`, `POST /audio/speakers/:verb`, `POST /audio/speakers/:verb/:address`; choosing one persists via `db.rs`'s `save_speaker_address`/`load_speaker_address` against a generic `player_settings` key-value table, and `Verb::Use` sends `Command::ReopenOutput` so the choice takes effect in the same request, not on a later restart `[PI3-UI-020]`. Sonos should be the same shape wearing different verbs, not a second design.

**`[GDE-SONOS-750]` SPEC011's `PathState { chosen: Option<SpeakerId>, ... }` is the design this is heading toward, but is not yet built** — `player/src/path.rs` today is `start(device: Option<String>, ring_capacity)`, migration step 1 of 4 (`[SPEC-APS-100]`). This plan does not require finishing that migration first; it adds one new, narrower persisted choice (`[GDE-SONOS-770]`) that a future `SpeakerId` enum could absorb cleanly, without blocking on work this document does not own.

---

## 2. Identity: RINCON, not IP — the same principle this project already applies to files

**`[GDE-SONOS-760]` A Sonos speaker's IP can change; its RINCON (UDN) cannot.** `[GDE-SONOS-020]` already read this pair's UDN (`RINCON_347E5CCAE44A01400`) directly from `ZoneGroupTopology`. Persisting the RINCON, re-resolving its current IP by discovery at each startup rather than trusting a cached address, is the exact same move `[SPEC-DF-035]` already makes for a library file — bind by durable identity, not by the address that happened to be true once. A stale IP after a DHCP lease change should read as "not currently found," never as a silent wrong-target play.

---

## 3. Data model — one new key, in the table that already exists

**`[GDE-SONOS-770]` `player_settings` already is the mechanism `[GDE-SONOS-740]` needs; add to it, do not build a new table.** Two new keys, same shape as `speaker_address`:

| key | value |
| :--- | :--- |
| `output_mode` | `'local'` \| `'sonos'` — which pipeline is authoritative right now |
| `sonos_target` | JSON: `{"udn": "...", "name": "Office", "last_ip": "192.168.67.56"}` |

`output_mode` is the exclusivity mechanism itself, not a policy layered on top of two independent flags — `[REQ]` "only one output type at a time" is true by construction because there is exactly one value, not two booleans that could disagree.

---

## 4. Discovery — SSDP, deduplicated to one entry per stereo pair

**`[GDE-SONOS-780]` SSDP M-SEARCH for `urn:schemas-upnp-org:device:ZonePlayer:1`, the standard UPnP discovery multicast, then one `ZoneGroupTopology#GetZoneGroupState` call per responder to resolve group membership** — exactly the query `[GDE-SONOS-020]` already validated live. Every member with `Invisible="1"` is filtered out before the list ever reaches the UI; the coordinator's own `RINCON` is what gets shown and stored, so a bonded pair appears once, under its room name, never as two selectable half-speakers.

**`[GDE-SONOS-790]` A discovery module, not a discovery script — `player/src/sonos.rs`, shaped like `bluetooth.rs`:**

```rust
pub enum Verb { Scan, Use, Forget, Status }
pub struct SonosSpeaker { udn: String, name: String, ip: IpAddr }
pub fn run(verb: Verb, target: Option<&str>) -> Result<serde_json::Value, String>;
pub fn discover(timeout: Duration) -> Vec<SonosSpeaker>;
```

No `Pair`/`Repair` — Sonos has no pairing step of its own the way Bluetooth does; `Scan` finds what is already broadcasting, `Use` selects and persists it.

---

## 5. Web routes — the same family, a new prefix

**`[GDE-SONOS-800]` Mirrors `/audio/speakers/*` exactly, at `/audio/sonos/*`:**

```
GET  /audio/sonos            -- discovered + previously-chosen (even if absent now), like /audio/speakers
POST /audio/sonos/scan       -- trigger discovery
POST /audio/sonos/use/:udn   -- select, persist, switch output_mode to 'sonos', ReopenOutput
POST /audio/sonos/forget     -- clear sonos_target, fall back to output_mode 'local'
```

`bt_reply`'s shape (`reopened`, `audible`, `output`) is reused as-is for these responses — one reply shape the settings panel already knows how to read, not a second one to learn.

---

## 6. The runtime switch — what "only one output at a time" actually does

**`[GDE-SONOS-810]` `Command::ReopenOutput` grows a branch on `output_mode`, rather than gaining a sibling command.** Reading `output_mode='local'`: today's unchanged path — open the local device, whatever Bluetooth/PipeWire routing already resolved. Reading `output_mode='sonos'`: **do not open a local output stream at all** — bring up the encoder + HTTP stream (`[GDE-SONOS-350]`, tapping the engine's post-mix output) if not already running, `SetAVTransportURI` once against the persisted (freshly re-resolved) coordinator, `Play`. Switching away from Sonos: `Stop` the coordinator (a courtesy, not a correctness requirement), tear down the encoder/stream, fall through to the local path. **The exclusivity is structural** — one branch executes, never both — not a runtime check for "is the other one already on."

---

## 7. The setup UI

**`[GDE-SONOS-820]` One list, in Settings, three kinds of row, one selection.** Extends the existing Settings panel (`#panel-settings`) rather than adding a fourth top-level panel — output selection is a setting, the same category `skip_fade`/`resume_save` already live in. Rows: the local/default device; every Bluetooth device `/audio/speakers` already returns (unchanged); every Sonos speaker `/audio/sonos` returns, discovered or remembered-but-absent (shown, disabled, with an honest "not seen on the network" label — the same "say so plainly" instinct `[REQ-LIB-190]` already applies to a flag that stops resolving). Selecting any row is a single POST to that row's own `use` endpoint; the panel does not need to know Sonos exists as a concept beyond "another row with a `use` link," the same way it does not specifically know about any one Bluetooth device today.

---

## 8. Platform notes — both hosts, mostly for free

**`[GDE-SONOS-830]` The discovery, SOAP, and stream-serving code is plain Rust networking — no platform fork needed.** SSDP multicast and HTTP both run identically via `tokio`/`std::net` on Windows and Linux; the one platform-specific detail worth testing explicitly is multicast socket setup (`SO_REUSEADDR` and interface selection differ in their exact ceremony between the two), not a reason to write two implementations.

**`[GDE-SONOS-840]` The encoder's build differs by platform, and both paths are already handled by the crate, not by this plan.** `[GDE-SONOS-550]` already found Linux cross-compilation needs `autoconf`/`automake`/`libtool` added to `build/Dockerfile.aarch64`. Windows native builds take `mp3lame-sys`'s *other* path — hand-compiled via `cc`, no autotools involved at all `[GDE-SONOS-540]` — so the Windows host needs nothing new beyond the C toolchain a Vaino dev build on Windows already requires today.

---

## 9. Feature gate

**`[GDE-SONOS-850]` `sonos`, default off, the same shape as `mpd` and `sampo-support`:**

```toml
sonos = ["dep:mp3lame-encoder"]
```

An appliance or desktop build that never asks for this carries none of it — no encoder, no SSDP listener, no new routes.

---

## 10. What SONOS001–007 already settled, reused outright

- Encoder: `mp3lame-encoder` `[SONOS005]`, static-linked `[SONOS006]`, LGPL housekeeping (`THIRD-PARTY-LICENSES`, one attribution line) `[SONOS007]` `[GDE-SONOS-710]`.
- Tap point: the engine's post-mix output, not a per-passage re-decode — preserves crossfade `[GDE-SONOS-350]`.
- RAM: well under 1 MB estimated, smaller than one open passage buffer `[GDE-SONOS-370..380]`.
- SOAP shapes (`SetAVTransportURI`, `Play`, `Stop`, `SetVolume`) — already validated live against the real Office pair while writing [SONOS001](SONOS001-appliance-survey.md).

---

## 11. Staged order

1. **Backend only, no UI**: `player/src/sonos.rs` (discovery + SOAP), the encoder tap, the HTTP stream route, feature-gated. Testable from the CLI/curl the same way `[SONOS001]`'s own survey work was done, against a fake or the real pair.
2. **Persistence + `Command::ReopenOutput` branch** (`[GDE-SONOS-770]`, `[GDE-SONOS-810]`) — output_mode becomes real, still no UI.
3. **Web routes** (`[GDE-SONOS-800]`) — curl-testable end to end.
4. **Settings panel UI** (`[GDE-SONOS-820]`) — the only step that touches skin.html/skin.js.
5. **License housekeeping** (`[SONOS007]` `[GDE-SONOS-710]`) — small, and easy to forget once playback works; sequenced last on purpose so it is not skipped once the feature "already works."

---

## 12. Open, going into implementation

1. **`[GDE-SONOS-860]` Whether `output_mode` belongs in `player_settings` permanently, or should migrate into a real `SpeakerId` once `[SPEC-APS-100]` step 2 (the trait + fake) actually happens** — this plan chose the cheaper, available mechanism now rather than block on unrelated, unfinished work.
2. **`[GDE-SONOS-870]` What happens to an in-progress crossfade when switching output mid-passage** — not addressed here; the honest default is "switching is a deliberate settings action, not expected mid-track," matching how a Bluetooth `Use` already behaves today.
3. **`[GDE-SONOS-880]` Whether `/audio/sonos` should also expose group/stereo-pair member details for display**, or whether "Office" as a single row is sufficient — a UI-polish question, not a correctness one.

---

**Traceability:** `[GDE-SONOS-740..880]` · derived from `[GDE-SONOS-010..730]`, `[SPEC-APS-060..100]`, `[SPEC-DF-035]`, `[REQ-LIB-190]`
