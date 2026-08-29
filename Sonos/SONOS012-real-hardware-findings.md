# SONOS012: Real-Hardware Findings — First Live Session Against the Office Pair

**Development Record — `Sonos`, 2026-08-29**

`[Sonos/SONOS010]` item 6 -- "never run end-to-end against the real Office pair" -- run for the first time tonight, at the user's own initiative. Five real bugs found and fixed live; a sixth, larger one found and **not yet fixed**, held for review because it touches the one code path this project has already been burned by once (`[REQ-AUD-142]`, the "nine minutes became sixty-six seconds" bug `[GDE-SONOS-1230]` below cites directly). This document is additive: nothing in `[SONOS002]`, `[SONOS008]`, `[SONOS009]`, `[SONOS010]`, or `[SONOS011]` is wrong so much as untested against a condition none of them anticipated -- a local output device that is not briefly absent but **entirely, indefinitely absent** for the whole session.

> **Related:** `[SONOS010]` item 6 · `[SONOS011]` · `player/src/sonos.rs`, `player/src/web.rs`, `player/src/engine.rs`, `player/src/path.rs`

---

## 1. What was actually run

The real Office pair (`RINCON_347E5CCAE44A01400`, a bonded stereo pair, `[SONOS001]`), reached from `vainopi` over the household's own WiFi, both paired Bluetooth speakers (`MIDDLETON`, `OontZ_Angle 3 U412`) powered off or out of range for the entire session -- not incidental, but the condition that exposed §3 below.

---

## 2. Fixed live, in order found

Each of these was reproduced, fixed, rebuilt, redeployed, and re-tested against the real pair before moving to the next -- the same discipline `[SONOS003]`'s repair log already models.

**`[GDE-SONOS-1180]` `SetAVTransportURI` was refused outright, HTTP 500, before Sonos ever attempted to fetch anything.** `soap::set_uri_and_play` sent `<CurrentURI>http://...</CurrentURI>` with empty `<CurrentURIMetaData></CurrentURIMetaData>` -- valid UPnP, refused anyway. Fixed by substituting the `x-rincon-mp3radio://` scheme for `CurrentURI` and supplying a minimal `object.item.audioItem.audioBroadcast` DIDL-Lite block naming the real `http://` URL in a `<res>` tag (`player/src/sonos.rs`, `soap::radio_uri`/`soap::radio_metadata`). Cross-checked afterward, not before, against SoCo's own `play_uri` (§4) -- the same substitution, confirmed independently rather than invented.

**`[GDE-SONOS-1200]` `Play` then failed the same way, and for a related reason: the stream `/audio/sonos/stream` route served a 404 until `ui.sonos` was populated, which only happened *after* both SOAP calls returned.** The coordinator resolves the URI as part of accepting `SetAVTransportURI`/`Play` themselves -- confirmed by the failure mode changing exactly when this was fixed, not merely reasoned about. `sonos::activate()` was split into `start_stream()` (ring + encoder, called first) and `point_and_play()` (the SOAP calls, called second), with `web.rs` populating `ui.sonos` between the two. A failed SOAP call now rolls `ui.sonos` back to `None` explicitly.

**`[GDE-SONOS-1210]` Fixing the above surfaced a deeper ordering issue: both SOAP calls then hung until timeout.** `Play` itself reads from the stream to confirm it before acknowledging -- and the engine was still only told to feed the ring *after* the SOAP call succeeded (`[Sonos/SONOS010 §2]`'s own fix), so there was nothing yet for that read to find. `Command::SetSonosRing(Some(ring))` now goes to the engine *before* the SOAP call, with the caller sending `SetSonosRing(None)` as an explicit rollback on failure -- keeping the actual property `[Sonos/SONOS010 §2]` wanted (the engine is never left believing a ring is wanted after a known failure) without the ordering that created this deadlock. `soap::URI_SET_TIMEOUT` was also raised from the module's ordinary 5 s to 15 s, since the coordinator's own confirmation read plainly needs more than that against this network.

**`[GDE-SONOS-1220]` The whole process SIGSEGV'd, roughly ten to fifteen seconds into the first activation that ever produced real, sustained audio.** `stream::encode_loop`'s output buffer was `Vec::new()` -- zero capacity, never grown, `.clear()`ed and reused every pass. `mp3lame_encoder::Encoder::encode_to_vec` writes into `output.spare_capacity_mut()`, so every call handed LAME's C encoder a zero-length buffer backed by a dangling, never-allocated pointer. Nothing crashed instantly only because LAME buffers several passes of lookahead before it has enough to emit a first real frame; once it did, it wrote through that pointer anyway. Fixed by reserving LAME's own documented worst case up front (`1.25 × samples_per_pass + 7200` bytes) once, before the loop, and never re-allocating (`.clear()` drops length, not capacity). **This path had never run with real, continuous audio before this session, in any test or any prior activation attempt** -- every earlier attempt failed at the SOAP layer before the engine was ever told to feed the ring at all, so this bug is exactly as old as `[SONOS009]`'s original implementation and was never exercised until tonight.

**`[GDE-SONOS-1190]` The loss-of-control watcher (`[Sonos/SONOS010 §3]`) tore down a session that had never actually been lost.** Confirmed directly: moments after a watcher-triggered fallback, `GetMediaInfo` against the speaker by hand still showed `CurrentURI` correctly pointed at Vaino. The watcher's own `current_uri` read used the module's ordinary 3 s timeout -- far tighter than the 15 s `[GDE-SONOS-1210]` just found this same speaker/network needs for other calls -- and any single failure to confirm was treated as a genuine takeover. Fixed with a longer read timeout (10 s) and a requirement of two consecutive failed polls, not one, before falling back.

---

## 3. Open: local-device backpressure starves *any* chosen output, Sonos included -- not fixed here

**`[GDE-SONOS-1230]` The root cause of "audible for a second or two, then nothing, with underruns climbing in bursts and the position clock stalling."** Confirmed by direct code reading, not yet fixed pending review -- this touches `mix_and_submit`, the exact function whose own comment already names a prior real bug (`[REQ-AUD-142]`: mixing more than the output could accept once silently dropped most of a nine-minute passage into sixty-six seconds of audio). A second mistake in the same function deserves more caution than the pattern tonight's other five fixes followed.

**The mechanism, in the code itself (`player/src/engine.rs`, `mix_and_submit`):**

```rust
let room = match &self.path.ring {
    Some(o) => {
        if self.out_room < Self::MIN_SUBMIT { self.out_room = o.free(); }
        self.out_room
    }
    None => self.scratch.len(),
};
let want = room.min(self.scratch.len()) / self.out_channels * self.out_channels;
if want == 0 || (want < Self::MIN_SUBMIT && self.path.ring.is_some()) {
    return 0;
}
// ... mix `want` samples ...
#[cfg(feature = "sonos")]
if let Some(r) = &self.sonos_ring {
    r.submit(&self.scratch[..filled]);
}
match &self.path.ring { Some(o) => { o.submit(...) } None => filled }
```

The amount mixed **every tick, for every output**, is sized to `path.ring.free()` alone -- the *local* device's own free space -- deliberately, per the function's own comment, as the mechanism that "propagates back-pressure" and prevents the exact `[REQ-AUD-142]` bug. `sonos_ring` is fed from the same `want`-sized mix, downstream of that same gate. **When `path.ring` reports no free space, nothing is mixed for anyone, Sonos included, no matter how healthy the Sonos ring or the actual chosen output is.**

`path.ring` reports no free space, sustained, in exactly the condition tonight's session ran in the whole time: a Bluetooth speaker that is not briefly absent but *permanently* absent for the session.

**Why, traced through `path.rs`:**

1. `watch()` (`path.rs`) checks for a dummy sink only while `playing` is true on the path supervisor's own copy of that flag, every `WATCH` (20 s). Finding one calls `out.mark_failed()`.
2. Every fallback *to* local -- the watcher's own `[GDE-SONOS-1190]` fallback, a failed activation's rollback, `sonos_forget` -- sends `Command::SetSonosRing(None)`, which calls `path.set_playing(true)` (assuming the session itself is still "playing"). `SetPlaying(true)` resets `watch_at` to *now*, forcing an immediate dummy check on the very next loop iteration -- which finds the dummy still there (nothing else has appeared) and calls `mark_failed()` at once.
3. Once `out.failed()` is true, `recover()` runs on its own backoff (`RETRY` = 2 s, doubling to `RETRY_MAX` = 30 s) **regardless of `playing`** -- `recover()`'s own signature takes `playing` only to decide whether to resume the stream on success, never to decide whether to run at all. Silencing local for Sonos (`set_playing(false)`) stops `watch()` from *starting* a new failure, but does **not** stop an *already-armed* `recover()` cycle from continuing to run.
4. Each `recover()` pass calls `out.release()` (`self.stream = None`, `SETTLE` = 700 ms deliberate wait) and then `Self::attach()` (open a new stream) -- and finds the dummy again, since nothing else exists, calling `mark_failed()` again, re-arming the same cycle. **`path.ring` has nothing draining it for the `release()`→`attach()` window of every single one of these cycles**, which recur indefinitely, on backoff, for as long as no real device appears -- which tonight was the entire session.

**This is not a Sonos-specific bug.** Sonos merely exposed it first, by being the thing someone was actively watching. The same gate stalls **local-only playback** identically whenever the Bluetooth speaker is absent for an extended stretch rather than the few seconds `[PI3-...]`'s existing retry design was written for -- the position clock and the decoders themselves would stall during every `release()`→`attach()` window, Sonos or no Sonos, which is very likely also the explanation for the pre-existing, previously-unremarked `output: N missed ring lock(s)` lines visible in `vainopi`'s own logs across ordinary nights, not only tonight's.

**Proposed fix, not yet applied -- for review before it touches this path:**

- **(a)** Treat a `released()` local device (`self.stream.is_none()`, mid-reopen) the same as a genuinely absent one (`path.ring == None`) for pacing purposes in `mix_and_submit` -- i.e. do not let a device that is *between* attempts, with literally nothing attached to drain it, report as "zero free space" in a way indistinguishable from "full and backed up." A device that is actually open and draining, even slowly, should keep pacing exactly as it does today -- `[REQ-AUD-142]`'s protection is about a real device that cannot keep up, not an absent one.
- **(b)** Gate `recover()`'s own retry loop on `playing`, the same flag `watch()` already checks, so a local device deliberately silenced for Sonos stops being hunted at all rather than continuing to cycle in the background. This alone would remove the interaction with Sonos; it would not address the identical stall local-only playback suffers when the chosen local device is the one that is absent.
- Both together address the user's own broader ask directly: *the player resumes gracefully when a selected output channel becomes available, or when an available one is selected* -- (b) stops fighting for a channel nobody currently wants; (a) stops a channel's own absence from stalling every *other* channel, including itself, once it is wanted again.

Neither is implemented yet. `[REQ-AUD-142]`'s own history is the reason: a pacing change in this exact function silently ate most of a passage once already, and a second attempt deserves the review this document exists to invite, not another late-night patch-and-redeploy cycle.

---

## 4. External reference research

Tonight's first five fixes (§2) were built from this project's own trained knowledge of the two integrations `[Sonos/SONOS002 §4]` already named as precedent (`node-sonos-http-api`, `SoCo`) -- not a live lookup at the time. Checked live afterward, for this document:

- **[SoCo `core.py`](https://raw.githubusercontent.com/SoCo/SoCo/master/soco/core.py), `play_uri`.** Confirmed the same `x-rincon-mp3radio://` scheme substitution this session built independently (SoCo's own `force_radio` parameter does exactly this: `uri = f"x-rincon-mp3radio{uri[colon:]}"`), and the same `object.item.audioItem.audioBroadcast` `upnp:class`. SoCo's own auto-generated metadata template additionally carries a `<desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">{service}</desc>` element Vaino's does not -- a Rincon service-registration descriptor. A single forum result found one concrete value in the wild (`SA_RINCON65031_`, TuneIn's own registered service id) but nothing confirmed for a plain, unregistered custom stream.
- **[Home Assistant `sonos/media_player.py`](https://raw.githubusercontent.com/home-assistant/core/dev/homeassistant/components/sonos/media_player.py), `_play_media`.** For exactly this case -- an arbitrary HTTP URL, not Spotify or a registered service -- Home Assistant's own integration (run across a far wider range of hardware and firmware than this project can test against) calls `soco.play_uri(media_id, force_radio=is_radio)` and **passes no DIDL metadata at all**. This downgrades the `cdudn` question from "likely missing requirement" to "probably not required for this case" -- a real, widely-deployed integration solves the identical problem without it. Not tried against the real pair either way tonight, since `[GDE-SONOS-1230]` (§3) is the better-evidenced explanation for tonight's actual symptom and was found first.
- **Not yet checked, worth it before another live session:** Home Assistant's own `sonos` component for how it handles a *coordinator that stops playing on its own* -- the exact `CurrentTransportState: STOPPED`-while-`CurrentURI`-still-correct state this session observed directly (§1's transport-state check, not yet reflected in `[Sonos/SONOS010 §3]`'s own watcher, which checks `CurrentURI` only and has no notion of transport state at all). A production integration handling the identical hardware for years has almost certainly already solved "the stream stalled and the speaker gave up" in a way worth reading before this project reinvents it.

---

## 5. Recommendations

1. **Fix `[GDE-SONOS-1230]` (§3) before the next live session**, both prongs -- it is the best-evidenced explanation for every symptom reported tonight (brief audio then silence, frozen position clock, bursty underruns), and it is not Sonos-specific, so fixing it benefits local-only playback on a flaky Bluetooth connection too.
2. **Extend the loss-of-control watcher to check `CurrentTransportState`, not only `CurrentURI`** -- tonight found a real gap where the coordinator stopped itself while still correctly pointed at Vaino, invisible to the current check.
3. **Read Home Assistant's own `sonos` component's stall-recovery logic** before the next attempt, per §4 -- the most likely single source of "how a production integration handles this speaker giving up mid-stream" this project has access to.
4. **Reconnect a physical Bluetooth speaker before the next test**, if practical -- it removes the exact condition (`[GDE-SONOS-1230]`) most likely to make any result ambiguous, letting a retest isolate whether Sonos itself is now solid.

---

**Traceability:** `[GDE-SONOS-1180..1230]` · found and fixed: `1180`, `1190`, `1200`, `1210`, `1220` · found, not yet fixed: `1230` · annotates `[Sonos/SONOS008 §6]`'s own mixer-independence claim, `player/src/engine.rs` (`mix_and_submit`)
