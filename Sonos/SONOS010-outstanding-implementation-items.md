# SONOS010: The Full List of What Is Not Yet Implemented

**Development Record — consolidated on `Sonos`, 2026-08-28**

Every gap named across [SONOS002](SONOS002-integration-options.md), [SONOS008](SONOS008-implementation-plan.md), and [SONOS009](SONOS009-implementation-record.md), plus two found by direct code inspection while answering a question about conflicting with Music Assistant — in one place, ranked by how much they matter rather than by which document first named them.

> **Related:** [SONOS009](SONOS009-implementation-record.md) §3 · `player/src/sonos.rs`, `player/src/engine.rs`, `player/src/path.rs`

---

## Correctness gaps — matter regardless of who is using this

**`[GDE-SONOS-1010]` Exclusivity is not actually enforced — the original requirement this whole feature was built against.** Confirmed directly against the code: `Command::SetSonosRing` only ever sets `Engine::sonos_ring`; nothing calls `path.set_playing(false)` on activation or `(true)` on deactivation. **Switching to Sonos output today does not silence the local device — both play simultaneously.** The fix is small and already has the right shape to reuse: `path.rs`'s existing `set_playing()` already feeds silence without closing the device (built for pause, for exactly this reason `[PI3-OPEN-020]`) — `sonos::activate` should call it with `false`, `sonos::deactivate` with `true`.

**`[GDE-SONOS-1020]` A failed activation leaves the engine in a half-started state.** `activate()` sends `Command::SetSonosRing(Some(ring))` *before* the SOAP call that can fail. If `SetAVTransportURI`/`Play` errors, the function returns `Err` and the caller reports failure — but the engine has already been told to feed a ring whose encoder thread just stopped (dropped along with the rest of `activate`'s local state). The ring fills and sits full; nothing is wrong externally, but the engine's own idea of state briefly disagrees with reality. Fix: send `SetSonosRing` only after the SOAP call succeeds, not before.

**`[GDE-SONOS-1030]` No detection of losing the speaker to another controller.** Raised directly in conversation: the integration is write-only. Music Assistant, the Sonos app, or an automation can call `SetAVTransportURI`/`Play` against the same renderer at any time — Sonos has no reservation concept, so whoever calls it last wins, silently. Vaino's own state would keep claiming "active" while the speaker played something else. This is precisely the failure mode `[SPEC-APS-030]` ("status must be observed, never inferred from execution") already exists to prevent for local/Bluetooth output, and was not carried over here. Fix: a periodic check, the same shape `path.rs`'s own `watch()` already uses for local output (~20s), re-reading `GetMediaInfo` and confirming `CurrentURI` still matches Vaino's own stream.

**`[GDE-SONOS-1040]` Vaino's own volume control does not reach the Sonos speaker at all.** `soap::set_volume` exists and was tested, but nothing calls it — `Command::SetVolume` only ever affects the local mixer's own gain, which the Sonos-side `OutputRing` receives already-mixed samples from regardless. Today there is no way to change a Sonos speaker's loudness from Vaino's own UI once it is the active output.

---

## Not yet exercised against reality

**Never run end-to-end against the real Office pair, still true as of `[GDE-SONOS-990]`.** Every SOAP shape was validated read-only during the survey; the actual write path — `SetAVTransportURI` + `Play` from Vaino, triggered live — has never been executed against physical hardware, deliberately, to avoid disrupting the Music Assistant path just repaired on the household's own speakers.

**Nothing re-activates a remembered Sonos target on startup, the gap `[GDE-SONOS-980]` already named.** `output_mode`/`sonos_target` persist correctly and `Use` activates live — but `vaino.rs`'s own boot sequence never reads the persisted choice back and calls `sonos::activate` before serving. A restart today silently falls back to local output even when Sonos was the last thing chosen, which is the gap the household actually asked about ("the player should remember the setup").

---

## Scope cuts, named on purpose

**The settings-panel UI is a separate, simpler block, per `[GDE-SONOS-1000]`.** No confirm/revert safety net (Bluetooth's exists because a wrong choice there can strand a listener with no way to hear whether it worked `[PI3-UI-030]`), no visual merge into one list, no stereo-pair member detail shown.

**Crossfade/mid-passage switch behaviour is undefined, per `[GDE-SONOS-870]`.** What happens to an in-progress crossfade when output mode changes mid-track was never specified or tested.

**No stereo-pair member detail in discovery results, per `[GDE-SONOS-880]`.** "Office" shows as one row; which two physical units make it up is not surfaced.

**`output_mode`'s long-term home is unsettled, per `[GDE-SONOS-860]`.** Living in `player_settings` today; whether it should migrate into a real `SpeakerId`/`PathState` once `[SPEC-APS-100]`'s own unfinished migration (trait + fake backend) happens is an open design question, not a bug.

**Encoder latency is unmeasured, per `[GDE-SONOS-290]` and `[GDE-SONOS-400]`.** RAM and binary size were measured for real `[GDE-SONOS-960]`; the actual added delay between a track change and audible sound on the Sonos speaker has not been.

---

## Recommendation

**`[GDE-SONOS-1050]` `[GDE-SONOS-1010]` first, alone, before anything else on this list.** It is the one item that contradicts an explicit requirement rather than merely leaving a convenience unbuilt, it is small (two `set_playing` calls), and every other correctness gap here (`[GDE-SONOS-1020]`, `[GDE-SONOS-1030]`, `[GDE-SONOS-1040]`) is independent of it and can be sequenced afterward in whatever order actual use surfaces as mattering most.

---

**Traceability:** `[GDE-SONOS-1010..1050]` · consolidates `[GDE-SONOS-860..880]`, `[GDE-SONOS-980..1000]`, `[GDE-SONOS-290]`, `[GDE-SONOS-400]`
