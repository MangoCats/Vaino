# SONOS013: The Icecast Experiment, and Two Live Incidents It Surfaced

**Development Record — `Sonos`, 2026-08-29**

Closes the deferred skip-lag gap `[SONOS012 §7]` and runs the transport-layer experiment `[SONOS012 §8]` recommended for the still-open reconnect gap `[SONOS012 §6]`. The experiment's own result is a clean negative -- valuable on its own -- but getting there surfaced two real, live incidents against the actual Office pair, both found, diagnosed, and fixed the same session: a false takeover from an out-of-band redirect, and a latent gap in the deploy tooling that briefly shipped a Sonos-less binary to production.

> **Related:** [SONOS012](SONOS012-real-hardware-findings.md) §6, §7, §8 · `player/src/engine.rs`, `player/src/web.rs`, `VainoPi/deploy.sh`

---

## 1. Skip lag closed (`GDE-SONOS-1270`)

`cut_ring_to_incoming` (`player/src/engine.rs`) now cuts `sonos_ring` the same way it already cut `path.ring` -- the same `begin_skip_transition` call, same overlay, same fade/lead milliseconds, gated behind `#[cfg(feature = "sonos")]` like every other `sonos_ring` touch in `mix_and_submit`. `[SONOS012 §7]` root-caused the lag to exactly this gap and deferred the fix to its own pass rather than fold it into an already-large session; this is that pass.

**Verified two ways.** `a_skip_cuts_the_sonos_ring_too` builds a real backlog in `sonos_ring` with nothing draining it (`PathHandle::silent()`, so `sonos_ring`'s own free space paces the mix), skips, and asserts the ring was actually truncated to the fade rather than left to drain in full -- fails without the fix, passes with it. Live against the real Office pair: a skip issued through `/command/skip` returned `204`, the history log recorded it correctly (`"kind":"skip"`, the outgoing passage at ~17% played), and the stream connection was undisturbed across it. **The functional correctness of the fix is confirmed live; the actual perceived latency improvement is not** -- that requires listening in real time, which no session tonight had a quiet enough window for. Carried forward as open, the same honest posture `[SONOS010]`'s own item 11 (encoder latency) already models for a measurement only the user can make.

---

## 2. The Icecast experiment: a clean negative on the reconnect gap (`GDE-SONOS-1300`)

**Goal, per `[SONOS012 §8]`'s own recommendation:** isolate whether the ~30 s reconnect cadence (`[SONOS012 §6]`) is caused by something in Vaino's own hand-rolled stream serving -- chunked-transfer framing, the `FakeLength` content-length claim, missing ICY metadata -- by putting a real, standards-compliant Icecast server in front of the identical audio and pointing the real Office pair at it instead.

**Setup:** `icecast2` (Debian package, preseeded via `debconf-set-selections` for a non-interactive install) on vainopi, port 8000. `ffmpeg` relayed Vaino's own `/audio/sonos/stream` into an Icecast mount (`/vaino.mp3`) as a second, non-disruptive subscriber -- the broadcast channel underneath already supports more than one reader (`player/src/web.rs`, `sonos_stream`'s own comment says as much), confirmed rather than assumed before relying on it. The real coordinator was then pointed at the Icecast mount via a raw SOAP call replicating `soap::set_uri_and_play`'s exact envelope (§3 below covers why a raw call, and what it cost).

**Result, five minutes of clean observation against the real pair:**

| Icecast connection opened | Gap from previous |
|---|---|
| 16:07:16 | (first) |
| 16:07:50 | 34 s |
| 16:08:23 | 33 s |
| 16:09:02 | 39 s |
| 16:09:33 | 31 s |
| 16:10:08 | 35 s |
| 16:10:47 | 39 s |
| 16:11:19 | 32 s |

**The same ~30-39 s cadence Vaino's own server produces, on a real Icecast instance.** This is a stronger test than the numbers alone suggest: Sonos's own User-Agent against the Icecast mount changed to `Linux UPnP/1.0 Sonos/86.8-78270 (ZPS12) Nullsoft Winamp3 version 3.0 (compatible)` -- the classic Shoutcast-handshake identification string -- confirming Sonos genuinely detected and used ICY/Shoutcast framing here, a different code path than talking to Vaino directly (plain `audio/mpeg` over `FakeLength`). It reconnected on essentially the same schedule anyway.

**What this rules out, cumulatively with `[SONOS012 §6]`'s own two prior negative results:** chunked-transfer encoding (ruled out there), the magnitude of a claimed content-length (ruled out there), and now ICY metadata / genuine Shoutcast framing (ruled out here). Combined with Music Assistant's own tracker report (`[SONOS012 §6]`, format-independent -- `.flac` and `.aac` both affected there), the transport/framing layer is now about as thoroughly eliminated as this project can practically manage. **The evidence points at a Sonos-firmware-side timeout specific to this class of stream, not at anything a serving-side change is likely to fix.**

`[SONOS012 §8]`'s recommendation is accordingly restated rather than retried a third way: the tractable target is narrowing the *gap itself* (buffer-ahead, fast re-request, something in the shape of Music Assistant's own "queue flow" mitigation), not eliminating the disconnect. No further transport-layer experiment is recommended without new evidence to justify one.

Icecast was stopped and disabled after the experiment (`systemctl stop/disable icecast2`) but left installed on vainopi, in case the "narrow the gap" work above ends up wanting a real Icecast in front of the stream permanently rather than as a one-off test.

---

## 3. Incident: an out-of-band redirect read as a takeover (`GDE-SONOS-1310`)

**Tried first, deliberately, as the cheapest possible version of the experiment: a raw SOAP call from a standalone script (`VainoPi/sonos-soap-redirect.sh`), no Vaino code change at all**, pointing the coordinator at the Icecast mount. It worked -- Icecast's own access log showed the real Sonos client connect and pull real audio.

**Within about twenty seconds, the existing loss-of-control watcher (`[SONOS010 §3]`, `GDE-SONOS-1030`) caught it and fell back to local output -- correctly, by its own rules.** `CurrentURI` no longer matched what Vaino itself had set, because the redirect happened outside Vaino's own knowledge; two failed confirmation polls later, the watcher did exactly what `[GDE-SONOS-1190]` built it to do. That correctness had a cascading cost nobody had reasoned through in advance: falling back tore down Vaino's own encoder (`SetSonosRing(None)` via `deactivate`), which killed `ffmpeg`'s input, which killed the Icecast mount's source -- and local fallback found only a dummy sink (no Bluetooth speaker present), leaving **nothing audible in the house for about a minute** before it was noticed (via `journalctl`, not by ear) and restored through Vaino's own normal `/audio/sonos/use/<udn>` activation.

**Fixed the direct way, not by weakening the watcher:** `POST /audio/sonos/redirect?url=...` (`player/src/web.rs`, `sonos_redirect`) points an already-active session's coordinator at a different URL without touching Vaino's own encoder or `/audio/sonos/stream` at all, so whatever is relaying it stays fed. It bumps `sonos_generation` the same way `sonos_use`/`sonos_forget` already do to retire a superseded watcher, then starts a fresh one via the existing `spawn_sonos_watcher` with the *new* URL as what "confirmed" means from here -- so a deliberate redirect and an actual takeover stay distinguishable, the property `[GDE-SONOS-1190]` already established for an ordinary session, now extended to a mid-session redirect too.

**Retried with the fix: clean.** The redirect landed, Sonos connected to Icecast, and the watcher logged nothing -- no `did not confirm`, no `falling back` -- for the full five-minute observation window in §2. Confirmed by direct inspection of `journalctl`, not inferred from the absence of a symptom.

**Lesson, stated plainly:** the loss-of-control watcher's own correctness (§3 here, `[GDE-SONOS-1190]`) means *any* future tool, script, or person that redirects the coordinator outside Vaino's own knowledge will trip it within about twenty seconds, with a cost (encoder teardown, local fallback) that is not obviously proportionate to a deliberate, expected change. `sonos_redirect` is the general answer for Vaino's own code; a human using the Sonos app directly, or Music Assistant, would still trip it exactly as designed -- which remains correct, since *that* case is a genuine takeover, not an experiment.

---

## 4. Incident: `deploy.sh` silently drops the `sonos` feature (`GDE-SONOS-1320`)

**Found while investigating why a routine `./VainoPi/deploy.sh` run -- used identically all session for the two prior fixes -- left the appliance with no Sonos support at all.** `sonos` is opt-in (`player/Cargo.toml`: `default = []`, deliberately, per its own comment -- a new authenticated-free network entry point on the appliance, not a weight concern). `deploy.sh`'s two `cargo build` invocations never passed `--features sonos`, and never had reason to notice: nothing compared what was about to be built against what the appliance was already running. `/audio/sonos` returning `404` immediately after a deploy that reported success was the only signal; the appliance had also lost its local fallback at that moment (no Bluetooth speaker present, the same "dummy sink" state `[SONOS012 §3]`'s own session ran in all night), so **for about four minutes nothing was audible in the house from any source at all**, until a manual rebuild with `--features sonos` and reinstall via `deploy-player.sh` restored it.

**Fixed at the tool, not just by remembering to type the flag next time:**

- `FEATURES` env var now reaches both `cargo build` invocations in `deploy.sh` -- the plain tree build directly, and the named-ref worktree build via `-e FEATURES` on the `docker run` (that build runs inside a heredoc script the host shell never expands, so the variable has to cross the container boundary explicitly).
- **A pre-flight guard, before Docker is even touched:** if the target host currently answers `/audio/sonos` with `200` and this invocation's `FEATURES` does not include `sonos`, the deploy refuses outright, with `ALLOW_FEATURE_DOWNGRADE=1` as the deliberate override -- the same escape-hatch shape as the existing `ALLOW_DIRTY=1`. Two `ssh`/`curl` attempts, not one, since a single transient failure reading as "not running Sonos" would make the guard silently useless on exactly the kind of flaky connection that most needs it -- observed directly this session (an unrelated transient `ssh` failure mid-cleanup, §3's own timing).

**Found and fixed live, not merely reasoned about, a second time within the same fix:** the guard's own first version used `--max-time 3` on the remote `curl` check, which raced `/audio/sonos`'s own live 3 s SSDP discovery scan (`sonos_list`) and silently read a Sonos-active host as not running Sonos -- caught by testing the guard against the real appliance before trusting it, not by inspection alone. Raised to `--max-time 8`. All three paths then confirmed live against `pi@vainopi`: refuses fast with `FEATURES` unset, proceeds with `FEATURES=sonos`, proceeds with `ALLOW_FEATURE_DOWNGRADE=1`.

---

## 5. What this evening's two incidents have in common

Both were caused by the same shape of gap: **a piece of automation (the watcher, the deploy script) correctly enforcing a rule it had no way to know this particular action was an exception to.** Neither the watcher nor `deploy.sh` was wrong on its own terms -- the watcher genuinely could not distinguish a deliberate redirect from a takeover without being told, and `deploy.sh` genuinely had no way to know the appliance's current feature set without being told to check. Both fixes follow the same shape as a result: not loosening the rule, but giving the automation the one additional fact it needed to apply its existing rule correctly. Worth naming as a pattern, since a third instance of it (some other action that looks like the thing a safeguard already exists to catch) is not obviously impossible.

---

## 6. Recommendations

1. **Confirm the skip-lag fix (§1) by ear** the next time there is a quiet listening session -- the functional fix is verified; the perceived latency is not.
2. **Do not re-attempt the reconnect gap (`[SONOS012 §6]`) at the transport/framing layer** without genuinely new evidence -- three independent negative results (chunked encoding, content-length magnitude, and now ICY/Shoutcast framing) plus Music Assistant's own format-independent tracker report make this a well-exhausted line of investigation. Pursue "narrow the gap" (buffer-ahead, fast reconnect) instead, per `[SONOS012 §8]`'s own restated recommendation.
3. **`sonos-soap-redirect.sh` (`VainoPi/`) is a standalone rollback tool, not the recommended path for a future redirect** -- `POST /audio/sonos/redirect` is, since only it keeps the watcher correctly informed. Kept for the case where Vaino's own web API is not reachable at all.
4. **If Icecast is revisited as a permanent front end** (rather than a one-off test) for reasons other than the reconnect gap -- multi-room broadcast, say, since Icecast supports concurrent listeners for free -- the relay and redirect mechanics here are already proven against the real pair and can be reused directly.

---

**Traceability:** `GDE-SONOS-1270` (closed) -- `player/src/engine.rs` (`cut_ring_to_incoming`, `a_skip_cuts_the_sonos_ring_too`) · `GDE-SONOS-1300` -- the Icecast experiment, transport/framing layer ruled out for `[SONOS012 §6]` · `GDE-SONOS-1310` -- `player/src/web.rs` (`sonos_redirect`), `VainoPi/sonos-soap-redirect.sh` · `GDE-SONOS-1320` -- `VainoPi/deploy.sh` (`FEATURES`, the pre-flight Sonos-downgrade guard) · commits `ee4f633`, `1d6f3f4`, `549ced9`
