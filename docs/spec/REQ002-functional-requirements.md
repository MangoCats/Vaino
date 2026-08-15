# REQ002: Functional Requirements

**Requirements Specification — Tier 1**

What Vaino and Sampo must do. **Supersedes [REQ001](REQ001-system-requirements.md)**, a v1 artifact retained only until mined `[GDE-DIS-010]`.

Derived from six years of MuLibPlay production behaviour `[GDE-BMK-*]` and McRhythm's refined functional work `[GDE-MCR-050]`, inherited as [MCR-REQ001](../inherited/mcrhythm/MCR-REQ001-requirements.md). McRhythm's *requirements* are inherited; its *architecture* is rejected `[GDE-CHT-050]`.

> **Domain acronyms** are chosen to avoid McRhythm's REQ namespace entirely (`PI, CF, NET, UI, VER, PB, OFF, NF, CTL, SEL, QUE, AUTH, TECH, IPD, FLV, ART, PERS, HIST, ERR, XFD, DEF, AF, UQ, OV`), so a `grep` for a Vaino requirement cannot match inherited material `[INH-HAZ-020]`.

---

## 1. Audio Playback — `AUD`

**`[REQ-AUD-100]`** Play the user's audio files with the **decoded audio stream unaltered** `[SPEC-DF-020]`. Verifiable, not merely asserted: `md5_encoded` before and after any Vaino operation must match.

**`[REQ-AUD-110]`** Decode by **streaming**, never whole-file. Bounded per-passage buffers `[GDE-ARC-050]`. The library contains a 244.9-minute file that needs ~2.6 GB decoded `[GDE-V1-030]`; this requirement is what makes it playable at all.

**`[REQ-AUD-120]`** Play any passage as a span of a larger file — a DAO file holds up to 40 `[GDE-BMK-020]` — without decoding the portions outside it.

**`[REQ-AUD-122]` Passage boundaries are sample-accurate, not packet-accurate.** A decoder seek lands on a container packet and reports where it actually landed; the remainder must be discarded so `start_ms` means `start_ms`. Measured drift when the reported landing was ignored: **648 frames, 14.7 ms**, on every passage with a non-zero start. It is inaudible in isolation, which is precisely the danger — it silently shifts every trim point, and because the passage length is measured from the *requested* start, it drags the end boundary along with it.

**`[REQ-AUD-130]`** Crossfade between consecutive passages using their lead-in/lead-out points and gain `[SPEC-SC-040]`.

**`[REQ-AUD-140]`** Resume playback state across restart, including position within a passage `[SPEC-SC-098]`.

**`[REQ-AUD-142]` Playback has exactly two states: playing and paused.** There is no "stopped". Pausing halts only the *consumer*; decoders keep filling their buffers, so resuming is instant and the pipeline stays primed after the initial power-on fill. A brief underrun at first start is therefore expected and acceptable; if it proves audible, the remedy is to prime the output before commencing, not to add a third state.

> **Halting the consumer means stopping the output device, not merely declining to submit.** The ring holds ~14 s and the device callback drains it regardless, so a pause that only stops submission leaves the music playing for another fourteen seconds — observed directly: reported position climbed 49.7 s → 51.7 s while paused. Stopping the stream leaves the ring full, which is what makes resuming instant. Where a backend cannot pause, the caller must be told rather than assume silence.
>
> **Underruns are counted only while playing.** A paused player underruns continuously by design; counting those buries the fault the number exists to expose. Before this distinction the idle figure reached 479,232 samples (~5 s); measured during real playback it is 0.

**`[REQ-AUD-150]`** Audio output is **co-located with the server**. Remote devices control; they do not receive streams.

**`[REQ-AUD-152]` Master volume is applied at the output device, in the callback** — not to samples on their way into the ring. Anything applied before submission is heard only after everything already submitted has drained, so with a ~14 s ring the control appeared to lag by ten seconds or more: the knob was governing audio computed far ahead of the ear. Applying it in the callback means a change reaches samples that are already buffered but not yet heard, which is precisely the audio a listener expects a volume knob to affect.

> This is the same buffer-depth trap as pausing by declining to submit `[REQ-AUD-142]`, and it recurs for any control the listener expects to act *now*. The rule generalises: per-passage properties (gain `[SPEC-SC-040]`, crossfade) belong before the mixer, because each side of a crossfade carries its own level; listening controls belong at the device.
>
> The value crosses to the callback as an atomic, not behind the ring's mutex. The callback must never block, and must be able to change level even on a tick where it cannot take that lock.

**`[REQ-AUD-154]` The master level is expressed in decibels, −72 dB to 0 dB**, `amplitude = 10^(dB/20)`. Loudness is perceived in ratios, so dB is the unit in which a listener's judgements are actually even; amplitude is not.

**The control is captioned with its own computed value** — "−32.0 dB", not "50 %". A percentage of travel is not a quantity the listener can act on. The figure displayed is the figure sent, so the caption cannot differ from the level in force.

| dB | amplitude | |
|---:|---:|:--|
| 0 | 1.000 | full scale |
| −6 | 0.501 | half amplitude |
| −20 | 0.100 | |
| −40 | 0.010 | |
| −72 | 0.00025 | bottom of travel |

> **There is no mute position.** An earlier revision reserved the very bottom of the travel for silence, following MuLibPlay, which closed hard below −8191 on its `-8192..=0` slider. Specifying the control's full range as −72…0 dB leaves no position for it. Nothing audible is lost: −72 dB is inaudible through any normal amplifier, and pause stops the output device outright `[REQ-AUD-142]`, which is the honest way to silence a player. If a detent below −72 is ever wanted, that is where it goes.
>
> The 72 dB span is the one figure here not inherited from MuLibPlay, which used 64 dB.
>
> **Amplitude is the internal representation; dB is the listener's.** The engine, the device and the saved resume point all speak amplitude — it is what multiplies samples. Only the control speaks in dB, converted at the edge of the HTTP layer.

**`[REQ-AUD-156]` The control's travel is quadratic in dB, flat where it meets full scale.** With `x` as travel from left (0) to right (1):

```
dB(x) = −72 × (1 − x)²
```

Zero slope at the top is what the curve is for: it spends most of the control's pixels on the top of the range, where listening actually happens, and compresses the bottom, which is inaudible anyway. On a 112-pixel control that is **0.007 dB per pixel** near full scale against **1.25 dB per pixel** at the far left. The control moves with single-pixel precision (`step="any"`) rather than in fixed increments.

| travel | dB | amplitude |
|---:|---:|---:|
| 0 | −72.0 | 0.00025 |
| 1/4 | −40.5 | 0.0094 |
| 1/2 | −18.0 | 0.126 |
| 2/3 | −8.0 | 0.398 |
| 3/4 | −4.5 | 0.596 |
| 1 | 0.0 | 1.000 |

> **The specification was over-determined, and the 2/3 point is what gave.** It asked for −72 dB at the left, −6 dB at 2/3, 0 dB at the right, and zero slope at the top — four conditions on a curve with three coefficients. Zero slope with `dB(1) = 0` forces the form `a(1−x)²`, and `dB(0) = −72` then fixes `a = −72`, putting 2/3 at `−72/9 = −8 dB`. Honouring −6 dB at 2/3 instead would raise the left end to −54 dB; fitting all three points exactly without the slope condition gives a curve that rises to +0.28 dB at x = 0.94 before falling back, which is non-monotonic and overshoots full scale. Both endpoints and the flat top are kept exact; the 2/3 point sits 2 dB low.
>
> **Displayed to a tenth of a dB, and sent as the displayed figure.** A tenth is far below what anyone can hear, so quantising to it loses nothing audible and guarantees the caption is the level in force. Near full scale the curve is flat enough that several adjacent pixels read the same figure — that is the zero slope behaving as specified, not a loss of precision.
>
> **This is the control's geometry, not audio, so it lives in the control.** The engine owns dB-to-amplitude and never sees a position; the browser owns position-to-dB and never sees an amplitude. The floor is sent to the browser rather than written there twice, so −72 exists in one place.

**`[REQ-AUD-164]` What is reported as playing is what is being *heard*.** A passage becomes current when its first sample leaves the ring for the device, not when the mixer starts on it — those are a ring's depth apart, so the display announced each track some fourteen seconds before it could be heard, cover art and all.

The test is `frames_mixed` against the ring depth, deliberately in **frames rather than position**: a resumed passage starts at a non-zero position and would otherwise announce itself the instant it was admitted. Skip is the exception and hands the display over at once, because it cuts the ring to the fade and the incoming passage really is audible within a second.

> **Measured:** resumed 40 s from the end of a passage, the title changed at **40.0 s** — exactly when the outgoing passage stopped reaching the device.
>
> **"Coming up" follows the same clock.** A passage leaves the queue when the mixer admits it, which is up to a ring's depth before anyone hears it — so the next track used to vanish from the list while the current one was still playing. Anything admitted but not yet audible now sits at the top of the list rather than being gone from it. Measured across a real handover: the next track stayed listed throughout, became uneditable 15 s before the change — the ring depth — and left the list at the moment it was heard.
>
> **What the mixer holds cannot be edited**, and says so `[REQ-VIS-185]`: its audio is already partly in the ring, so removing it from the queue would change nothing anyone could hear. The controls are disabled rather than absent, because a control that vanishes teaches less than one that explains itself.
>
> The reported passage outlives `live`, because a passage stays audible for a ring's depth after the mixer has finished with it. Blanking the display at that point would be the same fault mirrored.
>
> **This is the fourth instance of one fault** `[REQ-AUD-142]`, `[REQ-AUD-152]`, `[REQ-AUD-158]`. Pause had to stop the device, volume had to move into the callback, skip had to cut the ring, and now the display has to lag the mixer. The rule has earned its generality: **anything the listener perceives is downstream of a 14 s buffer, and anything measured upstream of it is measuring the wrong moment.**

**`[REQ-AUD-158]` Skip cuts the output ring short and fades what remains.** Dropping the passage upstream is not enough, and the code claimed otherwise: it discards the *decoder's* buffer, but the output ring still holds every sample already mixed. Measured, that was **14.0 s from button to new music** — the ring's full depth. The reported title changed in 0.5 s, so the display said one thing while the speakers said another for fourteen seconds.

**`[REQ-AUD-160]` The next passage is opened and decoded before anyone asks for it.** It is held in a prepared slot outside the mixer's `live` set — fed by the same decoder top-up, but not summed, so it is ready without sounding. Promotion is a move. Skip used to pay for a file open, a seek and a resampler build at the moment the button was pressed. The same slot serves ordinary crossfade admission, so the prepared path is the normal path and cannot rot from disuse.

**`[REQ-AUD-162]` A skip is a crossfade, not a stop followed by a start.** The outgoing passage falls away over `skip_fade_ms`; the incoming one begins its normal fade-in at `skip_lead_ms`; for the difference between them the two are **summed**. Both are adjustable while playing:

| | default | range | |
|---|---:|---|---|
| `skip_fade_ms` | 2.0 s | 0 – 10.0 s | outgoing fade-out |
| `skip_lead_ms` | 0.5 s | 0.1 – 2.0 s | when the incoming starts |
| *overlap* | *1.5 s* | | *the difference* |

A lead longer than the fade is legal and leaves silence between the two; the UI reports which of the three it is rather than letting a gap come as a surprise. The engine clamps, and the browser is sent the limits rather than keeping its own copy.

> **The fade is applied on the mixer thread, not in the callback** — the opposite of volume `[REQ-AUD-152]`, and for a reason worth stating: the incoming passage is summed into those same samples, so a fade-out evaluated in the callback would drag the newcomer down with the passage it is replacing. Cut, fade and overlay happen under one lock, so no callback can observe a ring that is cut but not yet faded.
>
> **The overlap is affordable only because the passage is already decoded** `[REQ-AUD-160]`. The 1.5 s laid over the outgoing tail is lifted straight from its prepared buffer, with its fade-in already applied on the way in `[XFD-ORTH-020]`, and through `mix` rather than by reading the ring directly so the accounting is identical to an ordinary tick.
>
> **Measured, on desktop hardware:**
>
> | | button to new music | underruns |
> |---|---:|---:|
> | drop the passage upstream only | 14.0 s | 0 |
> | cut the ring, 2 s fade, cold open | 2.5 s | 0 |
> | cut and overlay, 2 s fade / 0.5 s lead | **0.6 – 1.0 s** | 0 |
>
> Each figure includes up to 500 ms of snapshot-push granularity in the measurement itself. Clamping verified at both ends: 20 s → 10 s, 50 ms → 100 ms. HTTP stayed responsive throughout — median 12 ms, maximum 28 ms across ~2,700 requests spanning six skips.
>
> **Unverified on a Pi Zero 2W**, where the margin is far thinner: cutting to the fade length leaves the mixer only that long to refill ~13 s of ring.
>
> **Not persisted.** Both settings return to their defaults on restart. The resume row `[SPEC-SC-098]` carries volume but not these.
>
> **A passage part-way through an ordinary crossfade is discarded, not carried over.** It is already mixed into the ring alongside the outgoing one and is faded out with it; its decode ran a ring's depth ahead of the ear, so what the listener actually heard of it is nothing. The prepared passage takes its place.
>
> **The curve is `Exponential`, i.e. linear in dB** `[XFD-EXP-020]`. `Linear` and `Cosine` are equally available in [`fade.rs`](../../player/src/fade.rs) and the choice is one word; it has not yet been listened to.
>
> This was the third instance of one fault `[REQ-AUD-142]`, `[REQ-AUD-152]`: **a control the listener expects to act now cannot be implemented upstream of a 14 s buffer.** Pause had to stop the device, volume had to move into the callback, and skip has to reach into the ring. Any future control of this kind should be assumed to need the same treatment until shown otherwise.

> **Verification:** `[REQ-AUD-110]` is gated by [`memcheck`](../../player/src/bin/memcheck.rs), which decodes a passage of any length through the fixed-capacity buffer and **fails** above 150 MB peak RSS `[REQ-HW-100]`. It needs a long file from a real library, so `verify-targets.sh` runs it only when `VAINO_LONG_FILE` names one and reports **SKIPPED**, never passed, when it does not.
>
> This previously read "gated by an automated test playing the 244.9-minute file at ≤150 MB RSS and ≤500 ms skip latency". Two parts of that were untrue: nothing invoked the gate at all, and **no test measures skip latency** — `memcheck` does not, and the word now means the Skip control `[REQ-AUD-158]`, which is 0.6–1.0 s by design. `[REQ-AUD-120]` — playing a passage as a span of a larger file without decoding the rest — is exercised by the decoder's own tests and by every DAO passage the player opens, not by a dedicated gate.
>
> `[REQ-AUD-140]` verified end-to-end on desktop hardware (48 kHz device, 44.1 kHz sources, 8,079-passage library): a run interrupted at ~16 s saved 15.01 s, and the next run resumed the same passage at 15.0 s and went on to save 25.01 s — position advancing *from* the resume point, not restarting. The 15.01 s figure is also the check on audible-versus-mixed position: had the mixed figure been saved it would have read ~29 s.

> ### Known, accepted, and deferred
>
> Recorded from the review of 2026-08-14 so they are not rediscovered from scratch. Each was judged, not missed.
>
> **Position freezes briefly at a handover** *(cosmetic, accepted)*. When the passage being displayed leaves `live` before the next becomes audible, its reported position holds its last value instead of advancing. Bounded by the ring depth and invisible unless watched closely `[REQ-AUD-164]`.
>
> **A passage that fails to open is dropped by the engine but still counted as queued by the Director** *(correctness, outstanding)*. `prepare_next` advances past it while `note_queued` has already recorded it, so rotation history counts a passage that never played. Needs the engine to report dropped passages and the Director to forget them.
>
> **The display-name rule is stated twice** *(SSOT, outstanding)*. `QueueEntry::title` resolves MusicBrainz → tag → **filename**; the browse SQL resolves MusicBrainz → tag and then filters the rest out. An untitled, unidentified passage therefore plays under its filename but is absent from Browse. Measured at **0 passages** on the present library, so it is latent rather than active `[REQ-VIS-170]`, `[REQ-VIS-180]`.
>
> **The three skins each carry the same behaviour** *(DRY, outstanding)*. Volume drag handling, queue rendering and the fader conversion appear in all three; the programme `<select>` rebuild in two. Roughly 200 lines that belong behind optional binders in `core.js`, leaving a fourth skin nothing to reimplement `[REQ-VIS-160]`.
>
> **`publish()` makes presentation policy inside the audio engine** *(maintainability, outstanding)*. Which passage the listener is on, and how much queue a display gets, are display decisions living in `engine.rs` at 722 lines.
>
> **`web.rs` mixes routing, serialisation, browse, art and queue verbs** *(maintainability, deferred)*. Not urgent; treat as a priority at the next refactor, being the file most likely to keep growing.
>
> ### Recorded 2026-08-14, from the "what next" review
>
**`[REQ-LIB-160]` The listening is backed up; the library is not.** The library file holds two kinds of thing with opposite recovery stories. The **library** — files, passages, recordings, flavor — is derived from the audio on disk, and Sampo can grind it out again from nothing but time. The **listening** — 37,206 plays, 3,261 preferences, the programmes and their seeds — comes from years of a person using the thing, and nothing can reproduce it. Lose it and the Program Director is a random shuffle with opinions it can no longer justify.

Only the second is copied, and that choice is what makes the scheme work: **2.4 MB against a 553 MB library, 0.4%**. A backup small enough to take hourly is a backup that gets taken.

> **A copy, not a dump.** The output is a real SQLite file — openable, queryable, restorable by attaching it. A schema-and-INSERTs text dump needs a working player to be useful, and the moment a backup matters is the moment there isn't one.
>
> **Written under a temporary name and renamed.** Rename is atomic; a copy interrupted half way leaves a `.part` nobody will trust rather than a truncated file that looks fine.
>
> **The snapshot owns the connection and the library is attached `mode=ro`.** Two reasons, the second being the one that matters: `ATTACH` cannot create a database from a read-only connection, and this way a mistake in the copy cannot write to the thing being protected.
>
> **Grandfather-father-son retention**, because the value of an old snapshot is not that it is old but that it *predates whatever went wrong*. Damage noticed the same afternoon needs yesterday; damage noticed at Christmas needs March; a preference quietly corrupted two years ago needs a copy from before it. So: **one per day for seven days, one per month for twelve months, one per year indefinitely**, and always the newest whatever else happens. Within a period the latest is kept — it holds the most listening.
>
> Three years of six-hourly snapshots thin from 4,380 files to **20**: 10.5 GB to 48 MB. The yearly tier is unbounded on purpose; a decade of them is ten files.
>
> The date arithmetic is written out rather than imported — Howard Hinnant's civil-from-days, exact for every date this will see. Approximating a year as 365.25 days drifts a day a century and would silently file a snapshot under the wrong year, which is how the only copy of a year goes missing.
>
> **Never fatal.** A player that stops playing because it could not write a backup has turned a precaution into the fault. Failures are reported and playback continues.
>
> **A backup nobody has restored is a file of unknown value.** `restore_listener` puts one back, and **rehearsal is the default**: it reports exactly what a real restore would do and writes nothing until `--commit`. The numbers are the same either way, being measured from the same query before anything is written.
>
> **Passage ids are not stable; recording MBIDs are.** A Sampo rebuild renumbers passages, so restoring a history by its stored `passage_id` would silently reattribute years of listening to whatever songs hold those numbers now. Every play is re-pointed through its recording instead. Plays whose recording has left the library are **kept as they are** — a play that happened still happened, and discarding it to satisfy a foreign key would lose the only record of it.
>
> The whole restore is one transaction: half-applied would leave the listening in a state that never existed, which is worse than either version.
>
> **A safety copy is taken before committing, and is exempt from rotation.** The first version was not, and it very nearly destroyed what it was protecting: the safety copy and the snapshot being restored fell on the same day, the ladder keeps only the newest of a day, and the source was pruned out from under the restore. Safety copies now carry their own prefix and `prune` never looks at them.
>
> Verified end to end against a copy of the real library: 37,206 plays, damaged to 34,429, restored to 37,206, with 2 orphaned plays kept.
>
> Taken once at startup and hourly thereafter, on its own thread, off the audio path. `cargo run --example backup_now` takes one by hand — before a migration, or to check the thing works before trusting it to. Verified against the live 553 MB library **while Sampo was writing to it**: 37,206 plays copied, and the derived library correctly absent.

> ~~**Listener state has no backup, and it is not reproducible**~~ *(resolved by `[REQ-LIB-160]`)*. The library file holds 37,206 plays, 3,261 preferences, 8 programmes, 49 seeds and 24 occasion points, and the player writes to it continuously. Sampo can rebuild the library from the audio files; it cannot rebuild the listening history, and the Director is worthless without it. One interrupted write on a Pi takes all of it. The fix is small — a periodic snapshot through the SQLite backup API to a rotating file, the same mechanism the test copies already use.
>
> **Taste is unbuilt** `[REQ-PD-150]` *(feature)*. `listener_likes` holds nothing. It is the one substantial Director capability specified and not implemented, and `[SPEC-DIR-210/215/220]` are open design rather than settled, so it starts as a design conversation. Browse is its natural home: that is where a listener is looking at a track when they form an opinion about it.
>
> **Nothing starts the player on boot** *(deployment)*. The Dockerfiles are build targets, not deployment.
>
> **Errors are invisible on an appliance** *(operability)*. Sixteen `eprintln!` sites across engine, session, output and tags — decode failures, dropped passages, unstorable tag rows — all to stderr, on a headless machine with no terminal. Underruns and lock failures now reach the UI; the same treatment for recent faults would make them findable without a shell.
>
> **The HTTP surface has no authentication of any kind** *(security, decision needed)*. Anyone on the network can play, skip, reorder and browse the library. That may be right for a home LAN, but it should be a recorded decision rather than an accident.
>
> ~~**`skip_fade_ms` and `skip_lead_ms` do not survive a restart**~~ *(resolved by `[REQ-VIS-155]`, which found volume did not persist either)*.
>
> **Several audible choices have never been listened to** *(needs ears, not development)*: the skip fade curve `Exponential` against `Cosine` and `Linear`; whether 72 dB of fader travel gives enough resolution where the listening actually happens; whether 1.5 s of crossfade overlap on a skip reads as a transition or a muddle; and whether losing the mute detent at the bottom of the fader matters. Each is a one-word or one-constant change.
>
> **`session.rs` and `tags::backfill` have no tests, and nothing boots the server** *(test coverage)*. The absent integration test is the one that would have caught the queue-insertion and browse faults found by hand.

## 2. Program Director — `PD`

**`[REQ-PD-100]`** Select the next passage automatically, continuously, without user intervention. The queue never empties while eligible passages exist.

**`[REQ-PD-110]`** Implement MuLibPlay's weighting **as designed** `[GDE-PD-010..030]`: log-scale rotation, multiplicative artist-then-track eligibility, hard rotation block, linear recovery ramp, seasonal occasion multipliers, length bonus, and a `minWeightLimit` floor.

> **As designed, not as shipped.** This previously read "reproduce exactly". It changed when a variable shadowing was found in the shipped code: MuLibPlay's artist recovery ramp never reached the track weight, so a partially recovered artist has never damped its tracks `[SPEC-DIR-117]`. Vaino implements the ramp. MuLibPlay is a proven baseline, not a ceiling — six years of satisfactory listening is evidence the design is sound, not evidence that every behaviour of the binary is worth preserving.
>
> **Bit-identical reproduction is therefore no longer the acceptance test**, and could not be: the two now deliberately differ. The `GateOnly` coupling is retained so the divergence can be *measured* rather than assumed, which is the more useful check — it says how much the corrected ramp actually changes selection. Consistent with `[GDE-QUA-*]`, that measurement is diagnostic, never a pass/fail gate.

**`[REQ-PD-115]` Related recordings share a rotation.** Hearing a live take, a remaster or a compilation appearance suppresses the others `[SPEC-DIR-116]`. Every relation applies, each judged on its own play history, damped over a recovery window scaled by relation strength.

**`[REQ-PD-118]` Two master time scales — one for artists, one for tracks** — multiply every block and ramp duration `[SPEC-DIR-118]`. Range 0.0001–100.0000 to four decimal places, default 1.0000, at which they are exactly inert. They scale durations only, never weights, so *when* a passage becomes eligible is adjustable without touching *how much* it is wanted.

**`[REQ-PD-112]` Record every play, keyed by recording MBID.** Rotation is meaningless without it: an unrecorded play leaves a track as eligible as it was before, so a long session repeats what the algorithm exists to space out.

> Recorded at the **start** of playback, not on completion. Rotation spaces out what the listener has *encountered*, and a track skipped after ten seconds has been encountered — suppressing it for a while is the wanted behaviour. This also matches MuLibPlay, whose own note says the history structures update "as each new track finishes playing (or is put in the play queue)".
>
> Stored with `passage_id` **and** `mbid` `[SPEC-SC-095]`: the passage id is the convenience, the MBID is what survives a rescan that renumbers passages. An unidentified passage still records a play with a null MBID — it simply cannot contribute to rotation.
>
> A passage may legally hold a medley of several recordings, so the query selects the heaviest by a scalar subquery rather than a join, which would return that passage twice in every pool.

**`[REQ-PD-120]`** Select from **`radio` passages only** `[GDE-BMK-030]`.

**`[REQ-PD-130]`** Shape candidates by flavor distance `[SPEC-FD-040]` in two stages — prune against programme seeds, then order by similarity to the passage already queued — and apply randomness **last**, over the shaped pool `[GDE-PD-050]`.

**`[REQ-PD-140]`** Express a programme as **a list of exemplar passages**, not tuned parameters `[GDE-PD-040]`. "What should 10 AM sound like?" is answered by naming songs.

**`[REQ-PD-150]`** Honour user Likes and Dislikes as inputs to selection `[GDE-MCR-070]`. *How* Taste combines with seed shaping is open design `[GDE-OPN-030]`.

**`[REQ-PD-160]`** Degrade gracefully on partial flavor data: a passage with 11 known characteristics remains selectable alongside one with 71 `[SPEC-FD-040]`.

## 3. Visibility — `VIS`

Vaino's headline requirement `[GDE-CHT-030]`. Previously specified nowhere.

**`[REQ-VIS-100]` Why this track?** Every automatic selection exposes its full weight decomposition — artist weight, rotation block state, position on the recovery ramp, occasion multiplier, length bonus, distance to each seed, final rank, roulette position — **and the runners-up that lost**.

> **Status: the frequency half is delivered.** Every Director-chosen passage carries its full Stage-A decomposition — each term separately, never just the product — plus the five heaviest runners-up it beat, its share of the pool, and the pool's size and total weight. Written durably to `selection_decisions` and shown in the web UI. Terms sitting at ×1.0000 are dimmed rather than hidden: "this did not apply" is part of the answer.
>
> **Still missing:** distance to each seed, Taste effect, flow distance and roulette rank, all of which belong to stages B–D and need flavor distance `[SPEC-FD-040]`. Each stored record states which stages ran, so a decision recorded now cannot later be mistaken for a shaped one. A passage the Director did not choose — a resumed one, or one queued before the log began — reports that plainly rather than borrowing another passage's reasoning.

**`[REQ-VIS-110]` How was this identified?** Every ingest decision is a durable record: which stage matched, at what confidence, which candidates were rejected `[SPEC-SA-085]`. This is what converts an undocumented ritual `[GDE-BMK-050]` into a reviewable process.

**`[REQ-VIS-120]` Is this data trustworthy?** Every flavor value displays its provenance and measured accuracy `[SPEC-SC-070]`. A user must be able to see whether a value came from the dump, was locally computed at a stated error, or was entered by hand.

**`[REQ-VIS-130]`** Automatically computed boundaries, lead-in/lead-out points and gain are **reviewable and overridable** through a waveform view `[SPEC-SA-080]`. Manual edits outrank computed values permanently and are never silently recomputed.

**`[REQ-VIS-150]` The listening surface shows what is coming and what is in force.** The queue in play order, the active programme with a manual override `[SPEC-DIR-185]`, and master volume.

> Delivered 2026-08-13. Two implementation notes worth keeping:
>
> **Per-passage `gain_db` was read from the library and never applied to the audio** — it reached `QueueEntry`, was printed by `station`, and was silently dropped before the mixer. The library carries real values (median −3.0 dB), so tracks were playing at whatever level they were mastered at. It is now applied **per passage, before mixing**, so each side of a crossfade carries its own level; applying it after the mix would level the blend rather than the tracks, and the point is that they meet at a matched loudness. Master volume is applied last, over the mixed signal, because it is a listening level and not a property of any passage.
>
> **The Director is not `Sync`,** so the browser cannot reach it. Programme choice is written to a small shared cell that the engine reads on its next refill — an override therefore changes what is selected *next* rather than interrupting what is playing, which is the wanted behaviour. An unknown programme id is a 404 rather than a silent no-op.

**`[REQ-VIS-170]` A passage is named by MusicBrainz where MusicBrainz has an answer.** Three fields, three fallbacks, and every one of them says which source it came from `[REQ-VIS-120]`:

| shown | first choice | fallback | last resort |
|---|---|---|---|
| track | **Recording** title | file tag | filename |
| artist | **Artist** name, by credit | file tag | — absent |
| album | **Release** title | file tag | — absent |

**Recording and Release are different levels of the MusicBrainz model, and the distinction is the reason album is the hard one.** A Recording is a particular piece of recorded audio; its title names that performance. A Release is a published product — this pressing, this edition, this cover — and *its* title is what an album name is. One recording appears on many releases and one release holds many recordings, so the link is a join table rather than a column, and naming an album means choosing *which* release to name. That choice is ingest work, not playback work.

Artist and album have **no filename fallback**. Guessing a performer out of a path is how a library comes to believe in a band called "02"; absent is the honest answer.

> **Play counts are per recording, not per passage or per file.** The same recording reached through two files is the same thing heard twice, which is also how rotation already counts it `[SPEC-SC-095]`.
>
> **Measured on this library:** recording titles and artist names are present for the whole identified set — 7,912 recordings, 7,924 artist credits. `releases` and `release_recordings` are **empty**, so *every* album name today comes from the file's own tag. A sample of 40 files carried album on 40, artist on 40, title on 38, and embedded cover art on 28. The release tables are queried correctly regardless, so MusicBrainz album names take precedence the moment Sampo populates them, without a code change.
>
> **Cover art is read from the audio file, never fetched.** Playback must not depend on a live external service `[REQ-NEG-100]`, and the Cover Art Archive is exactly the dependency that forbids. It is served per passage at `/art/{id}` and cached for a day; a file with no picture is a plain 404, which is what lets a skin ask unconditionally and hide the element on failure. Roughly a third of this library has no embedded cover, so that path is the common case, not the exception.
>
> **Naming is not part of selection.** The Director loads the whole radio pool — 8,078 rows — and putting these five correlated subqueries in those columns would run them eight thousand times to answer a question that weighting does not ask. They are fetched for the dozen passages actually on screen instead, once each on the way into the queue, at under a millisecond apiece.

**`[REQ-VIS-180]` The library can be browsed by artist, by album and by track.** MuLibPlay's three "Browse by" pages, which were the one part of its interface Vaino had no answer for. Artist and album are ways *in* to tracks rather than destinations — an artist narrows to their albums, an album to its tracks, a track queues itself **next** — with a crumb trail so the narrowing is reversible.

**Browsing groups by the *displayed* name**, resolved exactly as `[REQ-VIS-170]` resolves it: MusicBrainz where it has an answer, the file's tag where it does not. What you can browse by is therefore precisely what you can see, rather than a second naming scheme that disagrees with the player.

> **The player builds its own tag index, in the background, on first run.** Album names come from the files' own tags and reading them takes ~18 s for 5,590 files — fine once, impossible per request. Doing it at startup on a spare thread is the difference between a feature that works and one that waits for someone to remember a command: the browse pages first shipped needing a manual scan, and came up empty for exactly that reason. It is incremental, so every later start is a no-op, and it is off the audio path entirely. `tagscan` remains for libraries prepared before they are ever played, and for `--all` after files are re-tagged.
>
> **Browsing never dead-ends on an artist.** An artist with no album names yet shows their tracks instead, with a note saying why. "No albums" is a useless answer to "show me this artist", and while the background scan is still running it is a temporary one as well.
>
> **The index is a cost worth naming.** Album has no source but the file's own tag, and reading it means opening and probing every file — 18 seconds for 5,590 of them, fine once and impossible per request. Re-scanning is safe and costs only the files added since.
>
> **Two handles write to the library, and neither is the audio path.** `PlayerStore` creates `file_tags` at startup alongside the resume row, and the background scan opens its own writable connection; `Library::open` stays read-only so the *reading* path cannot corrupt anything. The earlier claim that `tagscan` held "the only writable handle" stopped being true when the player learned to scan for itself.
>
> **Measured on this library:** 5,589 of 5,590 files carry tags and 3,604 carry cover art. Browsing yields 463 artists in 75 ms, 660 albums in 36 ms, and tracks in 80 ms — on demand rather than per tick, so a query is the right answer and a cache would be premature. Tracks are capped at 2,000 rows per response.
>
> **Built for a phone**, which is how these pages were actually used: an alphabet bar rather than a scrollbar, because 463 artists is a long way to drag with a thumb and one tap to the letter is the whole difference. Nothing depends on hover, no tap target is smaller than a fingertip, and the listing is rows rather than a table — a table on a narrow screen either scrolls sideways or crushes the name, which is the thing being looked for.
>
> **Letter headings are the jump targets**, so the bar cannot fall out of step with the list. They are derived exactly as the server sorts: strip a leading "The" for the heading while the `ORDER BY` does not, and "The Beatles" emits a stray B in the middle of the Ts.
>
> **A missing `file_tags` table is a failed query, not an empty result.** The first version shipped without creating it, so every browse page came up blank on a library that had never been scanned — and a blank page is indistinguishable from an empty library, which sent the fault-finding in the wrong direction entirely. The player now creates it at startup with its own writable handle, and the page reports a failure as a failure rather than rendering nothing.
>
> Without a scan, browsing by **artist** and **track** still works from MusicBrainz alone — 463 artists on this library. Only **album** is empty, and says why.
>
> **One page for every skin**, wearing the chosen skin's stylesheet. Three browse implementations would rot at different rates; this way a new skin gets browsing for free and can still restyle every part of it. MuLibPlay's three separate buttons still work, via `?kind=`.
>
> **Browsing runs off the engine entirely.** The page queries the database directly, so listing ten thousand tracks cannot interfere with playing one. Only the queueing action touches the player, and it inserts **next** rather than last: browsing to something and then waiting five passages for it is indistinguishable from the button not working. It does not interrupt what is playing — that is what Skip is for.

**`[REQ-VIS-195]` Tracks are selected, then acted on together.** A checkbox on each row and **one** set of Now / Next / Last for the list, disabled until something is ticked — a button that does nothing when pressed teaches nothing. Tapping anywhere on a row ticks it, because a 16-pixel checkbox is not a phone target and the whole row is.

Several tracks go in **exactly as one would**, and in the order they appear in the listing — so an album queues in its running order `[REQ-VIS-190]` in a single action.

> **They must arrive together.** Sent as separate requests, three passages inserted one at a time at the same place come out **backwards**; a whole album queued in reverse looks like a UI fault and is not. So the list travels as one request and is inserted by one command, and `Queue::insert_at` is the single place that knows how to keep an order. Both are tested, including insertion past the end.
>
> **A passage that cannot be read is dropped rather than failing the batch:** nineteen tracks queued beats none.
>
> Measured: all seven tracks of *Aja*, queued Next in one action, landed after the current passage as Black Cow → Aja → Deacon Blues → Peg → Home at Last → I Got the News → Josie.

**`[REQ-VIS-190]` An album opens in its own running order.** An album is a sequence, not an index: opened as one, its tracks belong in the order they were put on the record. Alphabetical remains right everywhere else, where a long list has to be findable rather than faithful.

Ordering is by disc, then track number, then title. **Unnumbered tracks sort after the numbered ones**, not ahead of them, which is where a bare `NULL` would put them.

> **The numbers come from the files.** MusicBrainz keeps position on the Release, in `release_recordings.position`, and those tables are empty `[REQ-VIS-170]` — so the file's own `TRACKNUMBER` is the only thing that knows an album's order. It is parsed for the forms tags actually use: `7`, `07`, and the `7/12` that ID3 writes and a naive parse drops, silently sorting a whole album alphabetically instead. Zero means absent, not first.
>
> **The tag index migrates itself.** An index built before track numbers existed has the rows but not the columns; adding a column succeeds exactly once, and on that run the stored tags are dropped so the background scan reads the numbers. Cheaper than a version table for one migration, and it cannot half-apply. Measured: an already-scanned library rebuilt itself in 18.7 s on the next start, with no manual step.
>
> **In album order the number leads the title and the alphabet bar disappears.** Letter headings over a running order would be neither monotonic nor meaningful, and an A–Z index over twelve tracks is furniture.

**`[REQ-VIS-185]` A found passage can be heard three ways, and the queue can be edited.** Wanting to hear something is not the same as wanting to hear it *instead* of what is playing, and one action has to guess which was meant:

| verb | what it does |
|---|---|
| **Now** | to the front of the queue, then skip into it — the only one that interrupts |
| **Next** | position 1 of the queue — the top of "Coming up" |
| **Last** | behind everything already waiting |
| **↑ / ↓** | one place sooner or later, clamped at the ends |
| **×** | out of the queue |

> **The queue holds only what is still to come.** The sounding passage is in `live` and is not in the queue at all, so index 0 *is* the next thing heard. Next shipped inserting at index 1 — one place too late — on the belief that the head of the queue was the playing passage. A test asserted that belief in as many words ("playing passage must stay at the head"), which is how it survived; that test now asserts the opposite and says why.
>
> **Now means the front, not "after the current".** Skip reaches for the front of the queue, so anything less would play whatever was already next instead — the passage the listener did not ask for.
>
> **Shifting clamps rather than wraps.** Nudging the first passage "sooner" does nothing, which is what is expected; wrapping it to last would be a surprise indistinguishable from a bug.
>
> **The three edits touch no database.** A queued passage is already in hand, so rearranging is a message to the engine and nothing more. Only the three library verbs read a passage in.
>
> **The controls sit to the left of the title, in fixed-width columns**, ordered × then ↑ then ↓. A column of identical buttons is one target to learn; buttons that shift with the length of a title are three. The list markers went with them — a number in front of the controls would put two unrelated things in the same column.
>
> **The controls are built in `core.js`, not in each skin.** All three want the same verbs on the same object; three copies would drift. A skin styles them through `.qedit` and decides where they go — it does not decide what they do. This replaces MuLibPlay's checkboxes and "Remove Checked" button, which took three taps to do what one now does.

**`[REQ-VIS-155]` What the listener sets, the player remembers.** Master volume, skip fade and skip lead survive a restart. They are written the moment a control moves rather than on the resume point's one-second timer: they change when a hand moves them and not otherwise, so saving them on that schedule would be a write per second to record that nothing had happened — and a setting that survives everything except a crash before the next tick is not really saved.

> **Volume already had a column and was never written to it.** The resume row saved position and playing state and quietly left the level behind, so it came back at full scale every start. That had been true since the row existed, and reads as "it persists" from the schema alone.
>
> Values from disk are clamped exactly as values from the network are — a number that has been sitting in a file deserves no more trust than one that just arrived.
>
> Verified across a real restart: −24.5 dB, 6 s fade and 1.2 s lead were set, the player stopped and started, and all three came back unchanged.

**`[REQ-VIS-160]` The listening surface is skinnable, and the skin is the only part that may differ.** A skin is three files — `skin.html`, `skin.css`, `skin.js` — and nothing else. It never opens a socket, never builds a URL, and never carries a copy of a control law.

What makes this possible was already true and merely tangled: **the server's contract is the snapshot and the command endpoints**, and the DOM was only ever one rendering of it. `core.js` holds that contract — the socket and its reconnection, the complete-snapshot dispatch, the command helpers, the shared formatting, and the fader curve `[REQ-AUD-156]`, which is specified rather than decorative and would be three chances to disagree with the engine if each skin carried its own.

| skin | what it is |
|---|---|
| `vaino` | The reference: quiet and typographic. Whatever a new skin needs from `core.js`, this one uses first, so a gap in the contract shows up here. |
| `mulibplay` | MuLibPlay's arrangement, with the colours and metrics taken from the page it actually serves rather than remembered. Stacked station buttons with the live one gold are the programme list; "Autoselect by clock time" is the manual override `[SPEC-DIR-185]`. |
| `winamp` | The awkward case, on purpose: a fixed-width appliance with bevelled chassis, green LCD, a scrolling title and a separate playlist window. It is the proof that the contract survives a skin that is not a document. |

> **The choice is per browser, not per player.** Two people on two phones may want different skins of the same radio, and neither should be able to restyle the other; it lives in `localStorage`, never in the engine. `?skin=` selects and sticks.
>
> **Skins are compiled in** (`include_str!`), so deploying to a Pi stays a copy rather than an install. Adding a skin is a row in `SKINS` and three files; the catalogue is served, so no existing skin needs editing to list a new one. An unknown skin or file is a 404 and nothing can reach outside the binary.
>
> **What the MuLibPlay skin cannot show, it does not invent:** album art, artist and album names, play counts, and the browse-by-artist pages are simply not in the snapshot. The omission is the engine's, not the layout's, and a skin fabricating them would be worse than the gap.
>
> **Verified** by [`build/verify-skins.js`](../../build/verify-skins.js), which also drives the browse page — its alphabet, its narrowing from artist to album to track, album ordering, the verbs refusing while nothing is selected, the selection travelling as one request in listing order, and a failed query reporting rather than rendering as an empty library `[REQ-VIS-180]`, `[REQ-VIS-195]`. For each skin it loads through `core.js`'s own loader into a real DOM and pushes snapshots at it — one with everything in it, one with almost nothing, optionally a live capture. It checks that nothing throws, that the transport is wired, and that dragging the fader to mid-travel posts `−18 dB`, which is the quadratic `[REQ-AUD-156]` confirming itself through the skin. Optional, because the player needs neither node nor jsdom to run: a skip is reported as a skip and never folded into the pass.

**`[REQ-VIS-140]`** Long-running operations report real progress and are interruptible without loss `[REQ-LIB-130]`.

## 4. Library Building — `LIB` *(Sampo)*

**`[REQ-LIB-100]`** Induct new music without hand labour — the reason the project exists `[GDE-CHT-020]`.

**`[REQ-LIB-110]`** Segment a multi-track file into passages and identify each against MusicBrainz `[SPEC-SA-070]`.

**`[REQ-LIB-120]`** Compute flavor locally, with no dependence on any live external service `[GDE-FEX-027]`. AcousticBrainz's API died within seven months of a successful bulk query `[GDE-MCR-045]`.

**`[REQ-LIB-130]`** Import is **incremental, resumable and interruptible** `[GDE-CHT-045]`. Adding a handful of tracks is the common case; a full library scan is the exception.

**`[REQ-LIB-140]`** Never re-decode audio to improve a classifier. Lowlevel features are cached permanently `[SPEC-SC-080]`.

**`[REQ-LIB-145]` Repair `duration_ms` from the decoded length at ingest, wherever it disagrees.** `[SPEC-SC-030]` already specifies "decoded, not header-claimed", and the migrated library violates it: **29.2% of files differ from their decoded length by more than 5 s**, 3.2% *over*-state it, and one overstates by **38.4 minutes** `[GDE-FEX-106]`.

> This is not cosmetic. Segmentation used the inflated value to create a **phantom passage** in a tail that does not exist, and the player uses `duration_ms` for lead-out timing. A field this load-bearing being wrong on a quarter of the library will keep producing symptoms that look like unrelated bugs — the extraction failure that surfaced it looked at first like an ffmpeg fault.
>
> Repair it where it disagrees, rather than always: `ffprobe` costs ~50 ms, but rewriting a correct value is churn. Passages already derived from a wrong duration need re-checking, not just the file row.
>
> **Done 2026-08-13** by `tools/repair_durations.py`, over all 5,590 files:
>
> | | |
> | :--- | ---: |
> | durations wrong by >1 s | **1,621 (29.0%)** |
> | …over-stating the file | 270 |
> | error: median / p95 / max | **35.0 s** / 234.6 s / 2301.3 s |
> | passage ends past the real audio, clamped | **453** |
> | phantom passages deleted | 1 |
>
> A **median error of 35 seconds** is not encoder rounding. The 29.0% measured over the whole library matches the 29.2% sample estimate `[GDE-FEX-106]` exactly.
>
> One consequence worth recording: `lowlevel_cache` is keyed `(audio_md5, start_ms, end_ms)` `[SPEC-SC-080]`, so clamping an end **orphans that passage's cached features**. The features were still correct — extraction had already clamped the range before analysing — so 212 rows were **re-keyed rather than re-extracted**, each matching exactly one cache row on `(audio_md5, start_ms)`. Radio-passage coverage is now **8,078 of 8,078**. Any repair that moves a passage boundary must consider the cache key, or it silently discards work.

**`[REQ-LIB-150]`** Relocate a moved or renamed library by content, not path `[SPEC-SC-035]`.

## 5. Portability — `PORT`

**`[REQ-PORT-100]`** A Vaino installation with **no Sampo** can receive derived data and use every advanced feature `[SPEC-DF-080]`.

**`[REQ-PORT-110]`** Derived data travels by embedded tag, per-file sidecar, or whole-database migration — one payload schema across all three `[SPEC-DF-065]`.

**`[REQ-PORT-120]`** Listener state **never** travels with music `[SPEC-DF-055]`. The transport carries facts about the music, never facts about the listener.

**`[REQ-PORT-130]`** Imported metadata is verified before trust: recompute `audio_md5` and discard encoding-scope claims that disagree `[SPEC-DF-070]`.

**`[REQ-PORT-140]`** Writing tags to a user's files requires informed consent, and uses temp-file → verify → atomic replace so a failed write cannot damage the library `[SPEC-DF-092]`.

**`[REQ-PORT-150]`** Listener state is exported automatically on a schedule, integrity-checked before rotation, retained generationally `[SPEC-DF-094]`. It is the only irreplaceable data in the system.

## 6. Appliance — `HW`

**`[REQ-HW-100]`** Run continuously on a Raspberry Pi Zero 2W (512 MB) — **≤150 MB RSS** `[GDE-MCR-020]`. MuLibPlay uses 171 MB on a 1.8 GB Pi 4 `[GDE-BMK-010]`; Vaino must fit a third of the memory.

**`[REQ-HW-110]` Reach first audio quickly on power-up — best effort, and bounded by the output profile.** Management services may start afterwards.

This is deliberately **not** an absolute target, because the audio output channel determines what is achievable `[IMPL-PROF-010]`. A Bluetooth sink must associate before any audio can flow, and that cost is inherent to the channel rather than to Vaino. Two consequences:

- **`[REQ-HW-112]`** Where an output channel imposes an unavoidable startup delay, that delay is **accepted for that profile only**. It must not be allowed to set the standard for profiles that do not share it.
- **`[REQ-HW-114]`** Profiles without such a delay — I2S DAC, USB DAC, HDMI — must be configurable for the faster boot, **sacrificing Bluetooth capability** to do so. Fast boot and Bluetooth are alternatives, not a compromise to be split.

**`[REQ-HW-120]`** Survive repeated hard power loss without database corruption.

**`[REQ-HW-130]`** The player is portable and reaches ARM. Sampo need not `[SPEC-SA-018]`.

**`[REQ-HW-140]` Desktop and server hosts are first-class targets, not a by-product of the appliance.** Vaino runs on Windows, Linux and macOS as an ordinary application `[GDE-CHT-045]`. The Pi Zero 2W is the *constraining* target, not the only one.

**`[REQ-HW-145]` Every supported target is tested, not merely compiled.** `build/verify-targets.sh` runs the suite on Linux x86_64, Linux aarch64 (under emulation) and the host. Compiling is not testing: an audit found aarch64 had only ever been *built*, Linux x86_64 never built at all, and the suite only ever *run* on Windows.

**`[REQ-HW-147]` At least one verification must use a real audio device.** A null sink reports no device rate and therefore cannot detect a sample-rate fault. This is not hypothetical: playback ran **8.8% fast — about 1.5 semitones sharp** — because a 48 kHz device met a 44.1 kHz library with the resampler unwired, and every prior test had used a null sink.

## 7. Non-Requirements

**`[REQ-NEG-100]`** Vaino does **not** stream audio to remote devices `[REQ-AUD-150]`, require any live external service at playback time, or modify audio data `[REQ-AUD-100]`.

**`[REQ-NEG-110]`** Sampo does **not** play audio, run on the appliance, or hold listener state `[SPEC-SA-100]`.

**`[REQ-NEG-120]` Neither Vaino nor Sampo integrates with personal-cloud accounts.** No Google Drive, Gmail, Calendar, or equivalent from any vendor — not for storage, not for scheduling, not for identity. This is a scope boundary, not an unimplemented feature.

> Three reasons it stays closed. **Playback must not depend on a reachable service** `[REQ-NEG-100]`; an appliance that cannot play music because a token expired has failed at its only job. **Network cost is justified per data class** `[SPEC006 §B]` — identification earns its lookups because MBIDs cannot be derived locally; nothing in playback, selection, or flavor can make that case. **Listener history is the user's** `[SPEC-DF-090]`, which is why class-D export exists at all; routing it through a third-party account inverts that.
>
> The near-miss worth naming: occasion weighting `[REQ-PD-050]` is seasonal, computed from month and day against the system clock `[SPEC003 §3.3]`. It is *not* a calendar integration and must not become one — reading real appointments would make selection fail when a remote service is unreachable.
>
> Off-machine backup of a class-D export to cloud storage is a legitimate thing a **user** may choose to do with a file Vaino has already written. Vaino does not do it for them, and nothing in the system may assume it happened.

---

## 8. Coverage Gaps

Areas where [MCR-REQ001](../inherited/mcrhythm/MCR-REQ001-requirements.md) has substantial requirements that Vaino has **not yet adopted or rejected** — recorded so the gap is visible rather than accidental:

| McRhythm area | Status |
| :--- | :--- |
| User identity / authentication (`AUTH`, 13 reqs) | Undecided. MuLibPlay is single-user with no auth. |
| Multi-user coordination (`PERS`, `UQ`) | Undecided; likely out of scope for v1. |
| Network status & offline operation (`NET`, `OFF`, 66 reqs) | Partially implied by `[REQ-LIB-120]`; not enumerated. |
| Three build tiers Full/Lite/Minimal (`VER`, 19 reqs) | Superseded by the Vaino/Sampo split `[GDE-ARC-010]`. |
| Error handling (`ERR`, 5 reqs) | Not yet enumerated. |

---

**Traceability:** `[REQ-AUD-100..NEG-110]` · supersedes `REQ001` · derived from `[GDE-BMK-*]`, `[GDE-PD-*]`, `[GDE-CHT-*]`, inherited `MCR-REQ001`
