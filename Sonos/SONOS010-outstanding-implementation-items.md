# SONOS010: The Full List of What Is Not Yet Implemented

**Development Record — consolidated on `Sonos`, 2026-08-28; nine of eleven items closed the same day by [SONOS011](SONOS011-closing-the-correctness-gaps.md); item 6 run for the first time the following day, per [SONOS012](SONOS012-real-hardware-findings.md)**

Every gap named across [SONOS002](SONOS002-integration-options.md), [SONOS008](SONOS008-implementation-plan.md), and [SONOS009](SONOS009-implementation-record.md), plus two found by direct code inspection while answering a question about conflicting with Music Assistant — in one place, ranked by how much they matter rather than by which document first named them.

> **Related:** [SONOS009](SONOS009-implementation-record.md) §3 · [SONOS011](SONOS011-closing-the-correctness-gaps.md) · [SONOS012](SONOS012-real-hardware-findings.md) · `player/src/sonos.rs`, `player/src/engine.rs`, `player/src/path.rs`

---

## Correctness gaps — matter regardless of who is using this

**`[GDE-SONOS-1010]` Exclusivity is not actually enforced — the original requirement this whole feature was built against. Closed by `[SONOS011]` §1.** Confirmed directly against the code: `Command::SetSonosRing` only ever set `Engine::sonos_ring`; nothing called `path.set_playing(false)` on activation or `(true)` on deactivation, so switching to Sonos output did not silence the local device — both played simultaneously. Fixed by reusing `path.rs`'s existing `set_playing()`, built for pause for exactly this reason `[PI3-OPEN-020]`.

**`[GDE-SONOS-1020]` A failed activation left the engine in a half-started state. Closed by `[SONOS011]` §1.** `activate()` sent `Command::SetSonosRing(Some(ring))` before the SOAP call that could fail; a failed call left the engine believing a ring was wanted after the encoder that would have fed it had already stopped. Fixed by sending `SetSonosRing` only once the SOAP call has actually succeeded.

**`[GDE-SONOS-1030]` No detection of losing the speaker to another controller. Closed by `[SONOS011]` §1.** Music Assistant, the Sonos app, or an automation can call `SetAVTransportURI`/`Play` against the same renderer at any time with no signal back — the write-only gap `[SPEC-APS-030]` already exists to prevent for local output. Fixed with a periodic `GetMediaInfo`/`CurrentURI` re-check, guarded by a generation counter against acting on a superseded session.

**`[GDE-SONOS-1040]` Vaino's own volume control does not reach the Sonos speaker at all. Closed by `[SONOS011]` §1.** `Command::SetVolume` only ever affected the local mixer's own gain. Fixed with a fire-and-forget `RenderingControl#SetVolume` call, mapped linearly from Vaino's dB range onto Sonos's `0..=100`.

---

## Not yet exercised against reality

**Run for the first time against the real Office pair, at the user's own initiative — `[GDE-SONOS-990]`'s gap, closed by [SONOS012](SONOS012-real-hardware-findings.md).** Six real bugs found and fixed live: an outright-refused `SetAVTransportURI`, a stream-fetchability race, a SOAP deadlock, a process-crashing encoder buffer bug, a too-eager loss-of-control watcher, and — held for explicit review before touching it, since it shares a function with a prior real bug (`[REQ-AUD-142]`) — local-device backpressure silently starving whichever output is actually chosen, Sonos included, whenever the local device is the one currently absent. See [SONOS012](SONOS012-real-hardware-findings.md) for the full account, including what was checked against SoCo's and Home Assistant's own Sonos integrations.

**Nothing re-activated a remembered Sonos target on startup, the gap `[GDE-SONOS-980]` named — closed by `[SONOS011]` §1.** `vaino.rs`'s own boot sequence now reads the persisted choice back and calls `sonos::activate` (after re-resolving it through discovery) before the web server starts serving, in the background so a slow or absent speaker never delays the first request.

---

## Scope cuts, named on purpose

**The settings-panel UI was a separate, simpler block, per `[GDE-SONOS-1000]` — closed by `[SONOS011]` §1.** Sonos speakers now render as rows in the same list Bluetooth devices use, with the identical confirm-or-revert safety net and stereo-pair member detail shown.

**Crossfade/mid-passage switch behaviour was undefined, per `[GDE-SONOS-870]` — closed by `[SONOS011]` §1.** Verified, not merely reasoned about: a dedicated engine test confirms choosing Sonos mid-crossfade disturbs nothing already live, the same guarantee `ReopenOutput` and a Bluetooth reconnect already gave.

**No stereo-pair member detail was shown in discovery results, per `[GDE-SONOS-880]` — closed by `[SONOS011]` §1.** "Office" showed as one row with no way to tell which two physical units made it up; the topology parser now surfaces every member, channel-labelled, and the merged settings panel (above) shows it.

**`output_mode`'s long-term home was unsettled, per `[GDE-SONOS-860]` — closed by `[SONOS011]` §1.** Replaced by a proper `SpeakerId` type in `path.rs`, compiled unconditionally so it round-trips regardless of which features a build carries — realising `[SPEC-APS-100]`'s own aspirational `PathState.chosen` shape without waiting on the rest of that migration.

**Encoder latency is unmeasured, per `[GDE-SONOS-290]` and `[GDE-SONOS-400]`, left open by the user's own choice.** RAM and binary size were measured for real `[GDE-SONOS-960]`; the actual added delay between a track change and audible sound on the Sonos speaker has not been. The user will measure this personally.

---

## What remains

**Item 11** (encoder latency) still requires a stopwatch against real hardware nobody has run yet, and remains entirely the user's own to measure — the one item left, of eleven.

---

**Traceability:** `[GDE-SONOS-1010..1040]` · consolidates `[GDE-SONOS-860..880]`, `[GDE-SONOS-980..1000]`, `[GDE-SONOS-290]`, `[GDE-SONOS-400]` · nine of eleven closed by `[GDE-SONOS-1060..1160]` in [SONOS011](SONOS011-closing-the-correctness-gaps.md) · item 6 run and closed by `[GDE-SONOS-1180..1230]` in [SONOS012](SONOS012-real-hardware-findings.md)
