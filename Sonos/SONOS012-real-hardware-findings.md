# SONOS012: Real-Hardware Findings — First Live Session Against the Office Pair

**Development Record — `Sonos`, 2026-08-29**

`[Sonos/SONOS010]` item 6 -- "never run end-to-end against the real Office pair" -- run for the first time this weekend, at the user's own initiative, across several sessions as each finding led to the next. Seven real bugs found and fixed live; an eighth symptom (§6) found, not yet diagnosed -- logs across the whole session showed nothing, so this round added the instrumentation the next listen needs rather than guessing at a fix. This document is additive: nothing in `[SONOS002]`, `[SONOS008]`, `[SONOS009]`, `[SONOS010]`, or `[SONOS011]` is wrong so much as untested against a condition none of them anticipated -- a local output device that is not briefly absent but **entirely, indefinitely absent** for the whole session.

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

## 3. Local-device backpressure starved *any* chosen output, Sonos included -- found, reviewed, and fixed

**`[GDE-SONOS-1230]` The root cause of "audible for a second or two, then nothing, with underruns climbing in bursts and the position clock stalling."** Confirmed by direct code reading before anything was changed -- this touches `mix_and_submit`, the exact function whose own comment already names a prior real bug (`[REQ-AUD-142]`: mixing more than the output could accept once silently dropped most of a nine-minute passage into sixty-six seconds of audio). A second mistake in the same function was held for explicit review before touching it, rather than folded into the same late-night pattern as the other five fixes -- reviewed, and fixed, once the mechanism below was traced all the way through rather than assumed.

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

**Fixed, in two parts -- both from the proposal this document held for review before touching this path:**

- **(a) `mix_and_submit` no longer trusts a `failed()` local ring's own `free()`.** A new `local_healthy` check (`Some(o) if !o.failed()`) decides what paces the mix: local's own free space when it is actually healthy and draining (unchanged from before -- by far the ordinary case, silenced for Sonos or not), the *Sonos* ring's own free space when local is failed but Sonos is chosen (a new `sonos_room()` helper), or the unconstrained discard-sink amount when neither exists to drain anything. Submission to a failed local ring at the bottom of the function drops the `debug_assert_eq!` that assumed local was always the pacing authority -- it now takes whatever of the mixed audio happens to fit, best-effort, since nothing is listening to it regardless.
- **(b) `path.rs`'s `recover()` now checks `playing` before doing anything at all**, the same flag `watch()` already gated on. An already-`failed()` local device stops being released and reopened on its own backoff the moment it is silenced for Sonos, rather than continuing to cycle in the background regardless of whether local is even wanted. The very next `SetPlaying(true)` -- Sonos giving up, a plain pause ending -- rearms `watch_at` immediately and recovery resumes exactly as before.
- **A third change, found necessary while fixing (a):** `mix_and_submit`'s own gate lived one level up from where this document first placed it. `tick()` itself only calls `mix_and_submit` at all when `self.path.audible()` is true (`[PI3-API-030]`'s own "nothing audible means nothing advances" refusal) -- and `audible()` answers for the *local* device alone. A chosen, healthy Sonos ring now counts as its own audible reason to proceed (`self.path.audible() || self.sonos_room().is_some()`), leaning on Sonos's own separate loss-of-control watcher (`[Sonos/SONOS010 §3]`) as the thing already responsible for noticing if that stops being true -- the same observed-not-inferred discipline `[PI3-API-030]` already asks of local output, on its own cycle. Without this third change, (a) alone was provably insufficient: a dedicated test proved `mix_and_submit` was never even being reached.
- **Deliberately NOT changed:** a failed local device with *no* Sonos ring either still stops the queue advancing, exactly as `[PI3-API-030]` already decided it should -- mixing on into a device known broken, with nothing else to hear it, is the "player that lies about what it is doing" that refusal exists to prevent. Only a second, independently-audible chosen output earns the override.

**Verified, not merely reasoned about:** two new engine tests, `a_failed_local_ring_does_not_starve_a_healthy_sonos_ring` and `a_failed_local_ring_alone_still_stops_the_queue_advancing`, construct a `mark_failed()` local ring directly and assert the two outcomes above against real mixing and real ring reads, not mocks. Both fail without the fix and pass with it -- confirmed by writing the fix in the wrong order, once, on the way here.

**Redeployed and retested live: audio is now continuous for the first time all night, but "skippy."** `[GDE-SONOS-1230]`'s own fix removed the starvation; a second, smaller gap it exposed remained. Traced to the same root idea one level further down: a real local device paces the whole mixing chain for free, because its own hardware callback only ever drains at exactly the audio's real rate. Nothing plays that role for Sonos. With local failed or silenced, `mix_and_submit` paces off `sonos_ring`'s own free space instead (§3, part a) -- and that ring holds roughly fifteen seconds, so it rarely reports "full," so the engine's own `submitted == 0` throttle (`vaino.rs`, the outer run loop) almost never fires, so the whole pipeline -- mixing, and `stream::encode_loop` behind it -- runs as fast as the CPU allows rather than at the audio's own rate. The LAME encoder then produces MP3 chunks faster than Sonos's own network read could drain them from the `CHANNEL_DEPTH`-deep (64) broadcast channel; the overflow is a `Lagged` error that `sonos_stream`'s `.filter_map(|item| item.ok())` silently drops. Continuous but with pieces missing is exactly what a dropped chunk in the middle of a live MP3 stream sounds like.

**`[GDE-SONOS-1240]` Fixed with `pacing_delay`, a substitute for the hardware callback local output gets for free:** `encode_loop` now tracks how much audio (in frames) it has actually encoded since it started, compares that against real elapsed wall-clock time, and sleeps whenever it is running ahead -- never emitting faster than the audio itself plays, regardless of how much backlog `sonos_ring` is holding. Pure and unit-tested directly (`running_far_ahead_of_real_time_asks_for_a_real_wait`, `already_behind_real_time_asks_for_nothing`, `an_unconfigured_sample_rate_never_panics_on_the_division`) rather than timed in an integration test, since the real timing this substitutes for is not something a fast test should wait on.

---

## 4. External reference research

Tonight's first five fixes (§2) were built from this project's own trained knowledge of the two integrations `[Sonos/SONOS002 §4]` already named as precedent (`node-sonos-http-api`, `SoCo`) -- not a live lookup at the time. Checked live afterward, for this document:

- **[SoCo `core.py`](https://raw.githubusercontent.com/SoCo/SoCo/master/soco/core.py), `play_uri`.** Confirmed the same `x-rincon-mp3radio://` scheme substitution this session built independently (SoCo's own `force_radio` parameter does exactly this: `uri = f"x-rincon-mp3radio{uri[colon:]}"`), and the same `object.item.audioItem.audioBroadcast` `upnp:class`. SoCo's own auto-generated metadata template additionally carries a `<desc id="cdudn" nameSpace="urn:schemas-rinconnetworks-com:metadata-1-0/">{service}</desc>` element Vaino's does not -- a Rincon service-registration descriptor. A single forum result found one concrete value in the wild (`SA_RINCON65031_`, TuneIn's own registered service id) but nothing confirmed for a plain, unregistered custom stream.
- **[Home Assistant `sonos/media_player.py`](https://raw.githubusercontent.com/home-assistant/core/dev/homeassistant/components/sonos/media_player.py), `_play_media`.** For exactly this case -- an arbitrary HTTP URL, not Spotify or a registered service -- Home Assistant's own integration (run across a far wider range of hardware and firmware than this project can test against) calls `soco.play_uri(media_id, force_radio=is_radio)` and **passes no DIDL metadata at all**. This downgrades the `cdudn` question from "likely missing requirement" to "probably not required for this case" -- a real, widely-deployed integration solves the identical problem without it. Not tried against the real pair either way tonight, since `[GDE-SONOS-1230]` (§3) is the better-evidenced explanation for tonight's actual symptom and was found first.
- **[Home Assistant `sonos/speaker.py`](https://raw.githubusercontent.com/home-assistant/core/dev/homeassistant/components/sonos/speaker.py) -- checked, for the *coordinator stops playing on its own* case.** No special stall-detection-and-resume logic exists there at all: `async_check_activity()` tracks reachability (a `ping()` against `AVAILABILITY_TIMEOUT`), and `SONOS_STATE_TRANSITIONING` events are explicitly ignored (`if new_status == SONOS_STATE_TRANSITIONING: return`) -- but nothing compares "expected to be playing" against "actually playing," and nothing distinguishes a deliberate stop from an unexpected one. Read as reference, not as a gold standard, per the instruction that sent this check back to the source: a project with access to real UPnP eventing (subscriptions) apparently still leans on plain reachability and a fallback poll (`SONOS_FALLBACK_POLL`) for this exact question, not a cleverer state machine. This is *reassuring* rather than merely inconclusive -- it suggests the actual stop tonight (§1, `CurrentTransportState: STOPPED` while `CurrentURI` stayed correct) was Sonos legitimately giving up on a feed that had gone quiet under it (§3's own mechanism), not a protocol subtlety Vaino was missing. No further Sonos-specific stall-recovery work is recommended on the strength of this alone; §3's fix is the more likely and better-evidenced remedy, and it is what got fixed.

---

## 6. A third symptom: periodic silence, invisible to every existing log -- instrumentation added, not yet diagnosed

**With both §3 fixes deployed, a longer listen (roughly four hours, on and off) found audio running clean for thirty to sixty seconds at a stretch, then a gap of silence lasting five to ten seconds, repeating.** Not the starvation of §3 (which was total and immediate) and not obviously the pacing gap of `[GDE-SONOS-1240]` (which read as brief dropouts within otherwise-continuous audio, not full silence) -- a third, distinct pattern.

**Checked first, and found wanting: the server's own logs across the whole session show nothing.** `active_udn` never left `Office`; the loss-of-control watcher never logged a fallback; no `SIGSEGV`, no panic, no "missed ring lock." Whatever is producing several-second gaps roughly once a minute is invisible to everything this project had already thought to log -- a real gap in the project's own observability, not merely an unsolved bug.

**`[GDE-SONOS-1250]` Three pieces of instrumentation added, aimed at the three places a gap like this could actually originate, none of them exercised or confirmed yet:**

- **`sonos_stream`'s own connection lifetime.** Logs `"stream connection opened"` when a GET arrives and, via a `LogOnClose` wrapper around the response body, `"stream connection closed after N.Ns"` the moment the client disconnects or the response is otherwise dropped. If the coordinator is periodically dropping and re-establishing its own read of `/audio/sonos/stream` -- a WiFi hiccup, a Sonos-side timeout on `x-rincon-mp3radio://` specifically, anything -- this is what would show it, and would point squarely at the network or at Sonos's own client behaviour rather than at Vaino's pipeline.
- **The broadcast channel's own `Lagged` errors, logged rather than silently dropped** (previously `.filter_map(|item| item.ok())`; now the `Err` arm reports how many chunks were lost before returning `None`). Distinguishes "the reader could not keep up with a healthy encoder" from the connection-drop case above.
- **`encode_loop`'s own starvation, past a 300 ms threshold.** Logs how long `ring.read()` returned nothing before audio resumed -- the signature of the mixing/decode pipeline itself falling behind real time, upstream of the encoder entirely, rather than anything in the encode-and-serve half of the pipeline `[GDE-SONOS-1240]` already covers.

**Deliberately not guessed further before this data exists.** A five-to-ten-second gap, once a minute, is equally consistent with a WiFi association hiccup on either end, a Sonos-side buffering policy this project has no visibility into, or a genuine periodic stall somewhere in Vaino's own pipeline still not identified -- and picking one to fix without the logs above to confirm it would be exactly the "throwing everything at the wall" this investigation has been asked to avoid. The next occurrence should log which of the three it is.

---

## 7. Recommendations

1. **Listen again with `[GDE-SONOS-1250]`'s instrumentation deployed**, and read back whichever of the three new log lines appears during the next gap -- that is what decides where to look next, rather than another guess.
2. **Extend the loss-of-control watcher to check `CurrentTransportState`, not only `CurrentURI`** -- still open, and lower priority: §4's own finding (Home Assistant has no special stall logic either) suggests the earlier `STOPPED`-while-correctly-pointed state was a legitimate reaction to a starved feed, which both fixes in §3 should now prevent from recurring, rather than a gap this watcher needed to paper over.
3. **Reconnect a physical Bluetooth speaker before the next test**, if practical -- it removes one remaining condition from ambiguity, and confirms local-only playback (which `[GDE-SONOS-1230]` also touched) still works exactly as before on the more ordinary night.
4. **Watch CPU load on `vainopi` during an extended Sonos session** -- a Raspberry Pi encoding MP3 in real time while also running the Program Director and everything else has less headroom than the development machine this was reasoned about on; `[GDE-SONOS-1250]`'s starvation log would catch this specific cause directly if it is the one at fault.

---

**Traceability:** `[GDE-SONOS-1180..1250]` · found and fixed: `1180`, `1190`, `1200`, `1210`, `1220`, `1230`, `1240` · found, instrumented, not yet diagnosed: `1250` (`player/src/web.rs`, `sonos_stream`/`LogOnClose`; `player/src/sonos.rs`, `stream::encode_loop`'s starvation log) · `1230` additionally touches `player/src/path.rs` (`recover`) and `[PI3-API-030]`'s own `audible`-gates-advancement rule in `player/src/engine.rs` (`tick`) · `1240` touches `player/src/sonos.rs` (`stream::encode_loop`, `stream::pacing_delay`) · five new tests (two engine, three pacing) pin `1230`/`1240` down · annotates `[Sonos/SONOS008 §6]`'s own mixer-independence claim, `player/src/engine.rs` (`mix_and_submit`)
