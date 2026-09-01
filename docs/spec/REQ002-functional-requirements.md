# REQ002: Functional Requirements

**Requirements Specification — Tier 1**

What Vaino and Sampo must do. **Supersedes REQ001**, a v1 artifact fully mined and deleted 2026-08-30 per `[GDE-DIS-010]` — its ideas either live here or are tracked as open questions in [GUIDE002 §6](../GUIDE002-rearchitecture-plan.md#6-open-questions).

Derived from six years of MuLibPlay production behaviour `[GDE-BMK-*]` and McRhythm's refined functional work `[GDE-MCR-050]`, inherited as [MCR-REQ001](../inherited/mcrhythm/MCR-REQ001-requirements.md). McRhythm's *requirements* are inherited; its *architecture* is rejected `[GDE-CHT-050]`.

> **Domain acronyms** are chosen to avoid McRhythm's REQ namespace entirely (`PI, CF, NET, UI, VER, PB, OFF, NF, CTL, SEL, QUE, AUTH, TECH, IPD, FLV, ART, PERS, HIST, ERR, XFD, DEF, AF, UQ, OV`), so a `grep` for a Vaino requirement cannot match inherited material `[INH-HAZ-020]`.

> **Related:** [SPEC023 Domain Vocabulary](SPEC023-domain-vocabulary.md) for what file/passage/recording/release/album/artist/track mean below — "recording" and "passage" are used precisely throughout; "track"/"song"/"album" appear only where they name a UI label or an informal sense, per SPEC023's own carve-out.

---

## 1. Audio Playback — `AUD`

**`[REQ-AUD-100]`** Play the user's audio files with the **decoded audio stream unaltered** `[SPEC-DF-020]`. Verifiable, not merely asserted: `md5_encoded` before and after any Vaino operation must match.

**`[REQ-AUD-110]`** Decode by **streaming**, never whole-file. Bounded per-passage buffers `[GDE-ARC-050]`. The library contains a 244.9-minute file that needs ~2.6 GB decoded `[GDE-V1-030]`; this requirement is what makes it playable at all.

**`[REQ-AUD-120]`** Play any passage as a span of a larger file — a DAO file holds up to 40 `[GDE-BMK-020]` — without decoding the portions outside it.

**`[REQ-AUD-122]` Passage boundaries are sample-accurate, not packet-accurate.** A decoder seek lands on a container packet and reports where it actually landed; the remainder must be discarded so `start_ms` means `start_ms`. Measured drift when the reported landing was ignored: **648 frames, 14.7 ms**, on every passage with a non-zero start. It is inaudible in isolation, which is precisely the danger — it silently shifts every trim point, and because the passage length is measured from the *requested* start, it drags the end boundary along with it.

**`[REQ-AUD-130]`** Crossfade between consecutive passages, `lead_in_ms`/`lead_out_ms` timing when the overlap is permitted and `fade_in_ms`/`fade_out_ms` `[SPEC-SC-046]` actually ramping gain across it, together with gain `[SPEC-SC-040]`.

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
> **Audited 2026-08-15**, because a debt list that reports finished work as outstanding is worse than no list: it spends the reader's attention on nothing and teaches them to distrust the rest of it. Every entry was checked against the code rather than against memory. Two had been resolved and are struck through with what settles them; two carried line counts that had drifted; one — the display-name rule — was found to be exactly right as written and left alone.
>
> **Position freezes briefly at a handover** *(cosmetic, accepted)*. When the passage being displayed leaves `live` before the next becomes audible, its reported position holds its last value instead of advancing. Bounded by the ring depth and invisible unless watched closely `[REQ-AUD-164]`.
>
> ~~**A passage that fails to open is dropped by the engine but still counted as queued by the Director**~~ *(correctness, resolved)*. `prepare_next` advanced past it while `note_queued` had already recorded it, so rotation history counted a passage that never played. The engine now collects them — `Engine::dropped`, pushed at both drop sites — and `take_dropped()` is drained in `session.rs`, which tells the Director to forget them. **Verified 2026-08-15**, by following the value from where it is pushed to where it is consumed.
>
> **The display-name rule is stated twice** *(SSOT, outstanding)*. `QueueEntry::title` resolves MusicBrainz → tag → **filename**; the browse SQL resolves MusicBrainz → tag and then filters the rest out. An untitled, unidentified passage therefore plays under its filename but is absent from Browse. Measured at **0 passages** on the present library, so it is latent rather than active `[REQ-VIS-170]`, `[REQ-VIS-180]`.
>
> ~~**The three skins each carry the same behaviour**~~ *(DRY, resolved)*. Volume drag handling, queue rendering and the fader conversion appeared in all three. They now live behind binders in `core.js` — `bindVolume`, `bindQueue`, `bindProgram`, `queueRow`, `showArt`, `named`, `badge` — and the three skins are 79, 166 and 85 lines, each calling into core ten to twelve times. **Verified 2026-08-15.** WinAmp remains the proof the contract is real: a fixed-width appliance with its own geometry and a scrolling title, needing nothing the document-shaped skins did not `[REQ-VIS-160]`.
>
> **`publish()` makes presentation policy inside the audio engine** *(maintainability, outstanding)*. Which passage the listener is on, and how much queue a display gets, are display decisions living in `engine.rs` — **1,088 lines** as of 2026-08-15, not the 722 first recorded.
>
> **`db.rs` and `web.rs` each mix several concerns** *(maintainability, deferred)*. `web.rs` was named first at **837 lines** — routing, serialisation, browse, art and queue verbs. **`db.rs` is now the larger problem at 1,635 lines**, holding the read path (`Library`), the write path (`PlayerStore`), the browse SQL and the identification-review logic. Most of that growth is the id-review work of 2026-08-15, so the entry that predicted "the file most likely to keep growing" was right about the shape and wrong about the file. `db/browse.rs`, `db/review.rs` and `db/naming.rs` fall out along seams that already exist.
>
> ### Recorded 2026-08-15: recording ids are not trustworthy
>
> **A sample of 2,000 radio passages, checked against the files' own tags** — evidence the ids did not come from:
>
> | | | |
> |---|---:|---|
> | agree on title and artist | 61.0 % | |
> | right artist, title differs | **33.9 %** | a mixture, see below |
> | right title, artist differs | 2.4 % | usually a credit difference |
> | agree on neither | **2.8 %** | plainly wrong |
>
> **2.8 % are simply the wrong song** — ~220 passages. Two examples sit next to each other: passages tagged *Magic Man* and *How Can I Refuse* by Heart carry MBIDs for *Breakdown* and *Learning to Fly* by Tom Petty. A whole album mis-assigned.
>
> **The 33.9 % is a mixture, and its shape is the diagnosis.** Some is legitimate naming variation — `Stoned Immaculate` against `Angels and Sailors / Stoned Immaculate`, `Karn Evil 9: 1st Impression, Part 2` against `Karn Evil 9`. But much of it is the **wrong track of the right album**: `Suffragette City` against `Ziggy Stardust`, `Woman` against `Happy Xmas (War Is Over)`, `Gemini Dream` against `Dr. Livingstone, I Presume`. Adjacent tracks, same artist.
>
> That pattern says the ids were assigned **by position within a matched album**. Any offset — a bonus track, a hidden track, an edition whose running order differs — shifts every id after it, which is exactly what MuLibPlay's migration would produce and what MCR-SPEC033's cascade `[AM-STG5-010]` exists to absorb.
>
> **Why this cannot be fixed by better metadata matching.** The ids may have been *derived* from metadata; checking them against metadata is checking a claim against its own source. Tags agreeing proves the derivation was self-consistent, not that it was right.
>
> **The reliable scheme is the audio itself** `[SPEC-SA-035]`, `[SPEC-SA-060]`. Chromaprint over the passage's decoded samples, looked up through AcoustID, returns the recordings that actually sound like this audio. It is independent of every tag, every filename and every album match, and it is the only evidence that is. The shape:
>
> 1. `fpcalc` over each passage's span — not the file, since a DAO rip is forty passages in one file.
> 2. AcoustID lookup, rate-limited and cached like the release fetch, keyed on `audio_md5` so a re-run asks nothing twice.
> 3. **Agrees** → the id is confirmed by evidence it did not come from. **Disagrees** → record both and the confidence, and prefer the fingerprint. **No match** → leave the id alone and mark it unverified; absence of a fingerprint is not evidence against one.
> 4. Every outcome to `ingest_decisions` `[REQ-VIS-110]`, because the whole point is that a listener can see which names are known and which are merely believed.
>
> Needs `fpcalc` (not installed here; ARM64 builds exist `[SPEC-SA-018]`) and an AcoustID application key. At one lookup per second the library is about two hours — the same order as the release fetch, and resumable the same way.
>
> **Until then, `[REQ-VIS-120]` matters more than it looked.** A name Vaino shows may be wrong, and the interface says nothing about how confident it is. Provenance display was already required; this makes it urgent.
>
> *Both done 2026-08-15. The fingerprint pass is `[REQ-LIB-165]` above — built on ffmpeg's chromaprint muxer rather than `fpcalc`, so the extra binary never became a prerequisite. Provenance is now shown in all three skins rather than as a tooltip in one, which is the same point made below.*

> ### Recorded 2026-08-14, from the "what next" review
>
**`[REQ-LIB-160]` The listening is backed up; the library is not.** The library file holds two kinds of thing with opposite recovery stories. The **library** — files, passages, recordings, flavor — is derived from the audio on disk, and Sampo can grind it out again from nothing but time. The **listening** — 37,206 plays, 3,261 preferences, the programmes and their seeds — comes from years of a person using the thing, and nothing can reproduce it. Lose it and the Program Director is a random shuffle with opinions it can no longer justify. Design decision and rationale in `[SPEC-DF-094]`; what follows is what was built to meet it, and how it was measured.

Only the second is copied, and that choice is what makes the scheme work: **2.4 MB against a 553 MB library, 0.4%**. A backup small enough to take hourly is a backup that gets taken.

> **A copy, not a dump.** The output is a real SQLite file — openable, queryable, restorable by attaching it. A schema-and-INSERTs text dump needs a working player to be useful, and the moment a backup matters is the moment there isn't one.
>
> **Written under a temporary name and renamed.** Rename is atomic; a copy interrupted half way leaves a `.part` nobody will trust rather than a truncated file that looks fine.
>
> **The snapshot owns the connection and the library is attached `mode=ro`.** Two reasons, the second being the one that matters: `ATTACH` cannot create a database from a read-only connection, and this way a mistake in the copy cannot write to the thing being protected.
>
> **Grandfather-father-son retention**, because the value of an old snapshot is not that it is old but that it *predates whatever went wrong*. Damage noticed the same afternoon needs yesterday; damage noticed at Christmas needs March; a preference quietly corrupted two years ago needs a copy from before it. So: **one per day for seven days, one per month for twelve months, one per year indefinitely**, and always the newest whatever else happens. Within a period the latest is kept — it holds the most listening.
>
> Three years of *hourly* snapshots, unpruned, would be 26,280 files — thinned by the ladder to **20**: ≈63 GB to 48 MB. *(Corrected 2026-08-30: this previously read "six-hourly snapshots... 4,380 files... 10.5 GB" — a stale figure from an earlier draft of the cadence, left uncorrected when the shipped schedule tightened to hourly below. The retained count, 20, is unaffected: it is set by the calendar ladder — 7 daily + 12 monthly + 1/year — not by how often a raw snapshot is taken.)* The yearly tier is unbounded on purpose; a decade of them is ten files.
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

**`[REQ-LIB-165]` Recording ids are checked against the audio, and a person settles the disputes.** Every recording MBID in this library arrived by one route: `source` on all 16,157 rows of `passage_recordings` reads `inherited:mulib`. They are therefore all exactly as good as one migration, and a wrong one is invisible — the player shows a real title by a real artist, and it is simply the wrong song. Nothing downstream can catch it, because everything downstream trusts the id.

> The design, the grading, the review interface and the measured results of the first full pass are in **[SPEC010](SPEC010-identification-review.md)**. Kept there rather than here because this register states *what must be true*, and that one records how it was established — 82.0% of the migrated ids confirmed by evidence they did not come from, and a review queue of 114 cards where a naive reading would have produced 1,433.
> ~~**Listener state has no backup, and it is not reproducible**~~ *(resolved by `[REQ-LIB-160]`)*. The library file holds 37,206 plays, 3,261 preferences, 8 programmes, 49 seeds and 24 occasion points, and the player writes to it continuously. Sampo can rebuild the library from the audio files; it cannot rebuild the listening history, and the Director is worthless without it. One interrupted write on a Pi takes all of it. The fix is small — a periodic snapshot through the SQLite backup API to a rotating file, the same mechanism the test copies already use.
>
> **Taste is unbuilt** `[REQ-PD-150]` *(feature)*. `listener_likes` holds nothing. It is the one substantial Director capability specified and not implemented, and `[SPEC-DIR-210/215/220]` are open design rather than settled, so it starts as a design conversation. Browse is its natural home: that is where a listener is looking at a track when they form an opinion about it.
>
> ~~**Sampo is specified as a separate project but ships inside this repository under Vaino's licence**~~ *(resolved 2026-08-15: relicensed in place)*. `tools/` now carries AGPL-3.0-or-later — `tools/LICENSE` verbatim from gnu.org, an `SPDX-License-Identifier` line in all 30 Python files so the terms travel with each file, and the root `LICENSE` scoped to say what it covers rather than implying everything. [`LICENSING.md`](../../LICENSING.md) sets out the arrangement and why the direction only works one way. Splitting the repository as `[SPEC-SA-010]` describes can still happen later without changing anyone's terms. Two things noted there and still true: `tools/` mixes Sampo's pipeline with research scripts and dev utilities, so the line to cut along is undrawn, and `check_docs.py` is AGPL only because of where it sits. The original entry follows.
>
> **Sampo is specified as a separate project but shipped inside this repository under Vaino's licence** *(licensing, was: decision needed)*. `[SPEC-SA-010]` says Sampo is *"a separate project: own repository, own licence, own platform envelope"*, and `[GDE-ARC-018]` sets the direction deliberately — Vaino MIT, Sampo AGPL-3.0, because Essentia is AGPL and MIT code may be incorporated into an AGPL work while the reverse is not true. In the tree as it stands there is **one `LICENSE`, and it says MIT**, covering `tools/` along with everything else.
>
> The *architectural* separation is real and holds: different languages, separate binaries, and `[SPEC-SA-015]`'s single channel — the shared SQLite file — with no linked code in either direction. What has not happened is the legal separation the specs describe. `tools/` also mixes Sampo's pipeline with research scripts and dev utilities, so the line to cut along is not yet drawn.
>
> Worth settling before anything is published rather than after: a licence is far cheaper to arrange than to re-arrange, and contributors who send patches to an MIT tree have been told something about the terms. The decision is genuinely open — split the repository as specified, relicense `tools/` in place, or record that the spec overreached and Sampo stays MIT because nothing AGPL is actually distributed here. Any of the three is fine; the current silent mismatch is not.
>
> **Nothing starts the player on boot** *(deployment)*. The Dockerfiles are build targets, not deployment.
>
> **Errors are invisible on an appliance** *(operability)*. Seventeen `eprintln!` sites across engine, session, output and tags (sixteen when first counted; the settings writer added one) — decode failures, dropped passages, unstorable tag rows — all to stderr, on a headless machine with no terminal. Underruns and lock failures now reach the UI; the same treatment for recent faults would make them findable without a shell.
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

**`[REQ-PD-110]`** Implement MuLibPlay's weighting **as designed** `[GDE-PD-010..030]`: log-scale rotation, multiplicative artist-then-recording eligibility, hard rotation block, linear recovery ramp, seasonal occasion multipliers, length bonus, and a `minWeightLimit` floor.

> **As designed, not as shipped.** This previously read "reproduce exactly". It changed when a variable shadowing was found in the shipped code: MuLibPlay's artist recovery ramp never reached the recording weight, so a partially recovered artist has never damped its recordings `[SPEC-DIR-117]`. Vaino implements the ramp. MuLibPlay is a proven baseline, not a ceiling — six years of satisfactory listening is evidence the design is sound, not evidence that every behaviour of the binary is worth preserving.
>
> **Bit-identical reproduction is therefore no longer the acceptance test**, and could not be: the two now deliberately differ. The `GateOnly` coupling is retained so the divergence can be *measured* rather than assumed, which is the more useful check — it says how much the corrected ramp actually changes selection. Consistent with `[GDE-QUA-*]`, that measurement is diagnostic, never a pass/fail gate.

**`[REQ-PD-115]` Related recordings share a rotation.** Hearing a live take, a remaster or a compilation appearance suppresses the others `[SPEC-DIR-116]`. Every relation applies, each judged on its own play history, damped over a recovery window scaled by relation strength.

**`[REQ-PD-118]` Two master time scales — one for artists, one for recordings** — multiply every block and ramp duration `[SPEC-DIR-118]`. Range 0.0001–100.0000 to four decimal places, default 1.0000, at which they are exactly inert. They scale durations only, never weights, so *when* a passage becomes eligible is adjustable without touching *how much* it is wanted.

**`[REQ-PD-112]` Record every play, keyed by recording MBID.** Rotation is meaningless without it: an unrecorded play leaves a recording as eligible as it was before, so a long session repeats what the algorithm exists to space out.

> Recorded at the **start** of playback, not on completion. Rotation spaces out what the listener has *encountered*, and a passage skipped after ten seconds has been encountered — suppressing it for a while is the wanted behaviour. This also matches MuLibPlay, whose own note says the history structures update "as each new track finishes playing (or is put in the play queue)".
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

**`[REQ-VIS-100]` Why this passage?** *(Renamed from "Why this track?" — [SPEC023](SPEC023-domain-vocabulary.md).)* Every automatic selection exposes its full weight decomposition — artist weight, rotation block state, position on the recovery ramp, occasion multiplier, length bonus, distance to each seed, final rank, roulette position — **and the runners-up that lost**.

> **Status: the frequency half is delivered.** Every Director-chosen passage carries its full Stage-A decomposition — each term separately, never just the product — plus the five heaviest runners-up it beat, its share of the pool, and the pool's size and total weight. Written durably to `selection_decisions` and shown in the web UI. Terms sitting at ×1.0000 are dimmed rather than hidden: "this did not apply" is part of the answer.
>
> **Still missing:** distance to each seed, Taste effect, flow distance and roulette rank, all of which belong to stages B–D and need flavor distance `[SPEC-FD-040]`. Each stored record states which stages ran, so a decision recorded now cannot later be mistaken for a shaped one. A passage the Director did not choose — a resumed one, or one queued before the log began — reports that plainly rather than borrowing another passage's reasoning.

**`[REQ-VIS-110]` How was this identified?** Every ingest decision is a durable record: which stage matched, at what confidence, which candidates were rejected `[SPEC-SA-085]`. This is what converts an undocumented ritual `[GDE-BMK-050]` into a reviewable process.

**`[REQ-VIS-120]` Is this data trustworthy?** Every flavor value displays its provenance and measured accuracy `[SPEC-SC-070]`. A user must be able to see whether a value came from the dump, was locally computed at a stated error, or was entered by hand.

> **Names carry their provenance visibly, in every skin** *(2026-08-15)*. It began as a tooltip in one skin of three, which is no use at all on a phone — the device this interface is mostly read from, and one with no hover. `named()` and `badge()` live in `core.js` so all three skins mark names the same way and each styles `.src` in its own idiom: gold for MuLibPlay, where gold already means "this is the one"; full-brightness LCD green for WinAmp, against half-lit for everything less certain.
>
> Only names that are *shown* are marked. WinAmp has no album line, so an album badge there would qualify something invisible, and its badges live in the stat row rather than the marquee — the marquee scrolls and is cloned to make the wrap seamless, so a badge inside it would drift off the panel and then appear twice.
>
> The badge is a separate element, so anything reading the name still gets the bare name. The check asserts the badges render, that at least two distinct sources appear, and that the name remains a text node of its own — and it was confirmed to fail when the display is reverted, which is the only way to know an assertion is doing anything.

**`[REQ-VIS-122]` MuLibPlay shows names bare** *(2026-08-17)*. The provenance
marks are removed from title, artist and album in that skin only. It is a
reproduction of the original's face, and `MB` / `tag` / `file` are a Vaino idea
the original never had.

This narrows `[REQ-VIS-120]`'s "in every skin", which was itself a deliberate
widening on 2026-08-15, so the reversal is recorded rather than quietly made.
The claim is not abandoned: Vaino and WinAmp still mark every name they show,
and the verifier now asserts the exemption *positively* — MuLibPlay must render
**no** badges, the others must still render theirs. A skin that is supposed to
mark names and silently stops therefore still fails.

**Known cost, accepted.** With `[REQ-VIS-124]` making MuLibPlay the default, a
browser that has never chosen a skin sees no provenance at all — which is the
state `[REQ-VIS-120]` was widened to escape. The name shown may be wrong, and
in that skin the interface no longer says so. Anyone wanting the answer changes
skin; the information is one selection away rather than absent.

**`[REQ-VIS-124]` A browser keeps the skin it chose; a new one gets MuLibPlay**
*(2026-08-17)*. The choice is per browser, not per player: two people on two
phones may want different skins of the same radio, and neither should restyle
the other.

Stored in `localStorage`, not a cookie. The server never needs to know which
skin a browser wears — the shell fetches it — so a cookie would ride on every
request, including the WebSocket upgrade and every art fetch, to carry
something the server does not read. Reads and writes are wrapped because
storage *throws* rather than returning null when a browser is in a private mode
or has site data blocked; unwrapped, that exception lands before any skin loads
and the page is blank rather than merely forgetful.

`?skin=` still overrides, and is remembered when used, so a link can hand
someone a skin and it sticks.

**`[REQ-VIS-130]`** Automatically computed boundaries, lead-in/lead-out points and gain are **reviewable and overridable** through a waveform view `[SPEC-SA-080]`. Fade-in/fade-out and their curves `[SPEC-SC-046]` are reviewable and overridable there too, though not automatically computed — every passage starts from the same fixed default. Manual edits outrank computed values permanently and are never silently recomputed.

**`[REQ-VIS-150]` The listening surface shows what is coming and what is in force.** The queue in play order, the active programme with a manual override `[SPEC-DIR-185]`, and master volume.

> Delivered 2026-08-13. Two implementation notes worth keeping:
>
> **Per-passage `gain_db` was read from the library and never applied to the audio** — it reached `QueueEntry`, was printed by `station`, and was silently dropped before the mixer. The library carries real values (median −3.0 dB), so tracks were playing at whatever level they were mastered at. It is now applied **per passage, before mixing**, so each side of a crossfade carries its own level; applying it after the mix would level the blend rather than the tracks, and the point is that they meet at a matched loudness. Master volume is applied last, over the mixed signal, because it is a listening level and not a property of any passage.
>
> **The Director is not `Sync`,** so the browser cannot reach it. Programme choice is written to a small shared cell that the engine reads on its next refill — an override therefore changes what is selected *next* rather than interrupting what is playing, which is the wanted behaviour. An unknown programme id is a 404 rather than a silent no-op.

**`[REQ-VIS-127]` The cover slot keeps its space, always** *(2026-08-17)*. The
element that holds cover art is a fixed box that is never empty, and never
leaves the layout.

It used to toggle `hidden` — `display: none` — around the load, so at every
track change the art left the flow, the page reflowed shorter, and reflowed
back when the next cover decoded. On MuLibPlay's 200&nbsp;px sleeves that threw
the controls beneath up and down the screen twice per track, and twice again
when the back cover followed. A passage with no picture simply stayed short, so
the resting height differed between tracks as well.

The cost is deliberate: a fixed box means a non-square cover letterboxes inside
it rather than the layout adapting to the image. Reserving space and adapting to
content are incompatible, and the jumping is the fault worth removing.

**`[REQ-VIS-128]` Covers cross-fade, and a missing one shows the kantele**
*(2026-08-17)*. Changing track fades between the outgoing and incoming sleeve
over one second. Two stacked layers, because swapping one element's `src` is
instantaneous and cannot be faded; the swap happens only once the incoming
image has **decoded**, since fading toward an image that has not arrived shows
an empty box for the length of the fade — the artefact being removed.

Where a passage has no embedded picture — roughly a third of this library, so
the ordinary case rather than an error — the box shows a **kantele**: Väinö is
Väinämöinen, and the instrument is his, alongside `Sampo` from the same source.
It is drawn from the instrument rather than from an illustration of one: the
strings terminate **on** the varras, the bar at the *narrow* end they are
knotted around, and run to tuning pins at the wide end. Traditional five-string
kanteles have no sound hole, the body being hollowed from beneath, so none is
drawn.

The mark is **inlined into the page, not set as an image source**, because a
data URI is an isolated document and cannot see `currentColor`. Inlined, one
mark takes each skin's own text colour — gold in MuLibPlay, LCD green in
WinAmp, dim grey in Vaino — and is correct in light mode without a second
asset. It also sits *under* both layers permanently, which is what satisfies
`[REQ-VIS-127]`: the box has something in it even before the first cover loads.

**`[REQ-VIS-170]` A passage is named by MusicBrainz where MusicBrainz has an answer.** Three fields, three fallbacks, and every one of them says which source it came from `[REQ-VIS-120]`:

| shown | first choice | fallback | last resort |
|---|---|---|---|
| track | **Recording** title | file tag | filename |
| artist | **Artist** name, by credit | file tag | — absent |
| album | **Release** title | file tag | — absent |

**Recording and Release are different levels of the MusicBrainz model, and the distinction is the reason album is the hard one.** A Recording is a particular piece of recorded audio; its title names that performance. A Release is a published product — this pressing, this edition, this cover — and *its* title is what an album name is. One recording appears on many releases and one release holds many recordings, so the link is a join table rather than a column, and naming an album means choosing *which* release to name. That choice is ingest work, not playback work — and it is frequently a choice with no wrong answer, since several release MBIDs often name the functionally same album; see [SPEC023](SPEC023-domain-vocabulary.md) and [SPEC010 §3](SPEC010-identification-review.md#3-searching-musicbrainz-directly).

**"Album" is not the passage `kind='album'` value `[SPEC-SC-040]`.** Same word, unrelated concept — a passage's `kind` is a playback-style choice (trimmed for rotation vs. full boundaries), never a claim about release identity. `[SPEC023]`'s "Album" entry is precise about the difference.

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

**`[REQ-VIS-186]` A queue entry is not a passage** *(2026-08-17)*. The edit
verbs name the **entry**, never the passage it plays.

A passage may sit in the queue more than once, deliberately, as a repeat. Those
are two entries that happen to name the same audio — and while the queue was
addressed by `passage_id` they were indistinguishable: **removing one removed
both**, and moving one moved whichever came first. Inherited from MuLibPlay,
where it behaved the same way, and reproduced here by carrying the same
identifier across.

Each entry is now stamped with a `qid` on its way into the queue, monotonic and
never reused. Never reused is the load-bearing part: an identifier a browser is
holding can then only be **stale** — naming an entry that has since played or
been removed, which is a quiet no-op — and never **ambiguous**, silently
addressing whatever took its place.

Two identifier spaces meet on `/queue/:ids/:action`, and conflating them was the
bug. `now` / `next` / `last` name **passages** in the library, because they add
something that is not there yet. `remove` / `sooner` / `later` name **entries**,
because they act on something already queued. Selection follows the entry too,
so two copies are separately pickable; the *explanation* is still fetched per
passage, since why a recording was chosen is the same for both copies.
>
> **Shifting clamps rather than wraps.** Nudging the first passage "sooner" does nothing, which is what is expected; wrapping it to last would be a surprise indistinguishable from a bug.
>
> **The three edits touch no database.** A queued passage is already in hand, so rearranging is a message to the engine and nothing more. Only the three library verbs read a passage in.
>
> **The controls sit to the left of the title, in fixed-width columns**, ordered × then ↑ then ↓. A column of identical buttons is one target to learn; buttons that shift with the length of a title are three. The list markers went with them — a number in front of the controls would put two unrelated things in the same column.
>
> **The controls are built in `core.js`, not in each skin.** All three want the same verbs on the same object; three copies would drift. A skin styles them through `.qedit` and decides where they go — it does not decide what they do. This replaces MuLibPlay's checkboxes and "Remove Checked" button, which took three taps to do what one now does.

**`[REQ-VIS-260]` The chosen speaker is remembered by Vaino, not guessed by a
script.** *(Fixed 2026-08-27.)* `use`/`pair` now write the address to
`player_settings`; the appliance's own reconnect timer (`vaino-speaker`,
`[PI3-AIM-020]`) reads it back instead of carrying one compiled into the
script. Reported as "playback is skippy" and traced from there: the timer had
no durable record of which speaker was current, so it kept paging a device
left over from early testing every 30 s. Paging a device the shared Bluetooth
radio cannot reach ties the radio up for several seconds, stalling whatever
*is* playing — audible as a skip, the on-screen position frozen for the
duration, and invisible to the player's own underrun counter, since the stall
never reaches the output ring at all. See [PI003 §1a](../../VainoPi/PI003-choosing-a-speaker.md#1a-what-the-listener-should-experience)
for the full account.

**`[REQ-VIS-255]` A programme is chosen against the listener's own clock, and
the control that reverts to automatic actually reverts.** *(Fixed
2026-08-24.)* Two faults reported together, both in service of
`[SPEC-DIR-185]`.

**The wrong programme was engaged.** `listener_settings.utc_offset_minutes`
governs what "local" means to `Programs::active` `[SPEC-DIR-180]`, and
nothing had ever written it — every library sat at the column's own default
of 0, so every programme was chosen against raw UTC clock time. Reported at
local 11:34 (UTC 15:34): Groove, which starts at 15:00, was on; Light, which
starts at 10:00 and should have run until Cool's 12:00, was not. The player
now asks the OS for its real offset once at startup, before the Director
reads it, and writes it back only when it disagrees with what is stored, so
a DST change self-corrects on the next restart instead of running an hour
off until someone notices. Not re-asked on an explicit library reload
mid-session -- rare enough, and startup already close enough behind it, that
the gap was left rather than opening a second writable connection to close it.

> **Not folded into `Programs::load` itself.** That function is exercised
> directly by a great many fixture-backed tests, every one of them relying on
> an absent `listener_settings` row reading as offset 0 to keep their
> time-of-day assertions independent of whichever timezone happens to run
> the suite. The OS ask lives in `PlayerStore::sync_utc_offset`, called once
> from `Session::open` -- deliberately not `Library::director()`, which was
> the first version's mistake: `Library`'s connection is opened read-only
> ("the player must not be able to corrupt the library"), so a write
> attempted there fails silently every time. It looked finished, ran on
> schedule, and never once reached the disk. `PlayerStore` is the one
> connection this process holds that can actually write.

**"Autoselect by clock time" re-checked itself the moment it was
unchecked.** The MuLibPlay skin only ever sent a command when the box
*became* checked (`Vaino.program('auto')`); unchecking sent nothing, so the
next snapshot — twice a second — read `program_manual` as still false and
put the tick back. Manual mode is a specific programme id, not a bare flag,
so there was never anything for a bare uncheck to mean. It now freezes on
whichever programme is engaged at the moment of unchecking, giving the
control a real, stable off-state instead of a dead end.

**`[REQ-VIS-250]` A play-history page, pageable and scrollable.** *(Built
2026-08-23.)* A third panel in the Vaino skin, opened the same way as
Settings and mutually exclusive with it: the most recently played and
skipped passages, newest first, each showing **title**, **artist**,
**album**, **what percentage of the passage was heard**, and **whether it
counted as a play or a skip** `[SPEC-PLAY-030]`. Paged at **10, 100 or
1000** rows, default **100**, with Prev/Next and a "page N of M" readout.

**Its own fetch, not the socket's snapshot.** A page of what has already
happened is not "what is true right now", and teaching the wire format to
paginate would serve nowhere else it is used. `GET /history?page=&size=`
reads `listener_play_history` and `listener_rejections` off the engine
entirely, the same way `/browse` does, so a long scroll back through
history cannot get in the way of playing the next track. A `kind='dequeue'`
rejection never appears here: it never sounded, so it is not a *play*
history `[SPEC-PLAY-050]`.

**The percentage is corrected, not frozen at the threshold.** `record_play`
still writes the moment half the passage (or four minutes) is crossed
`[SPEC-PLAY-030]`, so a crash right after still counts the play — but the
figure written then is only the threshold just reached, not what was
actually heard. The engine corrects that row once the passage is actually
done sounding. A skip writes its final figure directly, since a skip never
leaves anything draining behind it to wait for.

> **Corrected again, 2026-08-24: "departs" is not "is heard".** The first
> version wrote the correction the instant the passage left `live`, which is
> the moment its DECODER is exhausted — up to a ring's depth (`BUFFER_FRAMES`,
> ~15 s here) before its last sample reaches the speaker `[REQ-VIS-240]`. A
> track played all the way through therefore read as ~94%, never 100%,
> however completely it was actually listened to: the figure was frozen at
> "decoded", the same mistake the ring's-depth fix already corrected for the
> position display, made again in the one place that fix did not reach.
>
> The correction is now **held until the clock says the drain is done** — the
> same `(position, instant)` pair `draining` already carries for the display,
> read again for this. A skip or a seek that wipes the ring out from under a
> still-draining correction takes whatever it had reached as final, rather
> than leaving it waiting for a tail that will never arrive.

> **Absent, not zero.** `heard_ms`/`span_ms` are new columns on both tables,
> migrated onto an existing library the same way `id_reviews` gains its
> columns — `ALTER TABLE ... ADD COLUMN`, ignored where it already exists.
> A row written before this shipped has neither, and reads as an absent
> percentage (`—`) rather than a claimed 0%, the same distinction
> `counts_as_play` already draws for an unknown span `[GOV-SRC-040]`.

**`[REQ-VIS-240]` The position runs to the end of the track, not to the end of
the mixing.** *(Fixed 2026-08-23.)* The elapsed time and the progress bar stopped
about fifteen seconds short of every track and sat there until the next one
began.

A passage leaves the mixer when its decoder is exhausted, which is a ring's
depth — about 15 s here — before its last sample reaches the speaker. The
display already knew it had to cover that window and kept the *title*; it looked
the position up in the list of passages being mixed, which is the one place the
passage had just been removed from, so the number stopped.

**It is advanced by the clock, and that is the point.** The obvious measure —
what was mixed, less what is still buffered — is wrong during a crossfade,
because the incoming passage is filling that same ring and its depth says
nothing about how much of the outgoing one is left. What is left is simply time,
and audio plays at one second per second. Capped at the passage's own end, so
the clock cannot run past the music however long it sits there.

> Present since 2026-08-14 and reported by a listener, not by a test — the kind
> of fault that is invisible from a terminal and obvious from a chair.

**`[REQ-VIS-235]` The Vaino skin shows where the sound is actually coming
from.** *(Requested 2026-08-23; not built.)* Immediately below the State and
Underruns display: the **system path and filename** of the file being played,
with the passage's **start offset**, **end offset**, and the **total audio
length of the file**.

**Why it is worth the space.** Everything else on that page names what Vaino
*believes* it is playing — title, artist, album, all resolved through
MusicBrainz. None of it names the bytes. When a listener hears something other
than what the page says, the first question is whether the player is wrong about
the metadata or about the file, and there is currently nothing on screen that
separates the two. That question cost an hour on 2026-08-23 `[PI-CHR-080]`:
the audio was Genesis exactly as displayed, and the speaker was listening to
another device entirely — a fact a path on screen would not have proved, but a
start offset inside a 244-minute capture would have made the alternative
explanations checkable in seconds.

**Start and end matter as much as the name.** A third of this library is
passages inside long captures `[SPEC-MPD-052]`, where "playing Aqualung.mp3" is
almost no information: the same file holds fourteen passages across 64 minutes.
`2552 s → 2710 s of 3840 s` says where in it the needle is, and makes a
mis-trimmed span visible as a number rather than as a listening complaint.

**Three of the four are already in hand.** `QueueEntry` carries `path`,
`start_ms` and `end_ms`. The file's own length is **not** on it —
`QueueEntry::duration_ms()` is the passage span, and the file's is
`files.duration_ms` in the library. Carrying it means widening the entry where
`Library::passage` builds it, which is the honest place: the queue entry is what
crosses to a backend `[SPEC-BK-030]`, and a length it does not carry is a length
the far side cannot show either.

> **The path names the listener's filesystem**, and it should stay as local as
> the browser it is drawn in. It belongs in the snapshot the local UI reads and
> nowhere that travels `[SPEC-DF-055]` — a file path is already the weakest of
> the three identities `[SPEC-DF-030]` and the only one that is nobody else's
> business.

**`[REQ-VIS-230]` The underrun count can be restarted, and says what it counts
from.** *(Requested 2026-08-23; not built.)* A **Restart** button beside the
Underruns label in the Vaino skin, and **"since {date time}"** to the right of
the count. Clicking it zeroes the displayed count and captures the moment.

**Why it is wanted.** The count is cumulative for the life of the process, so it
answers "has this ever glitched" and cannot answer "is it glitching *now*". A
number that only ever grows stops being read: after a rough hour, a clean day
looks identical to another rough one. Restarting it turns the display into a
question about the present, which is the question a listener actually has.

Three things fall out of the shape, and are the reason this is written down
before it is built rather than discovered during:

* **The underlying counter is not reset.** It keeps running, per process, as it
  does today — the button moves a **baseline**, and the display shows the
  difference. Resetting the real counter would throw away the one number that
  answers the other question, and would mean the diagnostic lied to whoever was
  not looking at the button.
* **"Since" has an answer before anyone clicks.** It starts as the moment the
  process began, so a fresh player reads *since 09:14* rather than *since
  never*. That is also the honest label for the count it is showing.
* **A baseline cannot outlive its process.** The counter starts at zero on every
  start, so a baseline persisted across a restart would subtract a number that
  no longer exists and show a negative or absurd figure. It is therefore held in
  memory and re-seeded at startup — deliberately *not* in `player_settings`
  `[SPEC-SC-099]`, which is where a setting would otherwise go.

> **Startup is not a fault.** The count includes the ring filling for the first
> time as the device opens — 346,674 samples within seconds of launch, measured
> on the appliance `[PI-CHR-050]`. A restart button is also the cure for that:
> one click after startup and the display describes listening rather than
> booting.

**`[REQ-VIS-225]` The progress bar is a control, not only a display.** Clicking
anywhere along it moves to that point in the passage — the one thing a listener
reaches for that MuLibPlay never had, and that the Vaino skin drew but did not
answer.

**Click only, deliberately.** Dragging would mean a continuous stream of seeks,
and each one on the local engine costs a file open, a seek and a resampler
build. A click is one of those; a drag is one every frame.

**It lands alone.** Mid-crossfade both passages go and the sought one returns by
itself, because resuming an overlap the listener has just left behind is not
what they asked for.

**Offered only where the live backend can honour it** `[SPEC-BK-040]`. The bar
is marked as a control from the backend's own `seek` capability, so a side that
cannot seek shows a plain display rather than a control that does nothing.

**And seeking is not listening** `[SPEC-PLAY-012]`. The distance travelled earns
no credit toward a play — the accounting had to change from position to time
heard before this could be built at all.

> Both skins get it: the Vaino skin's existing bar becomes clickable, and the
> MuLibPlay skin gains one full width under the volume reading. The arithmetic
> lives in `core.js`, so the two cannot drift.

**`[REQ-VIS-220]` Vaino may write lyrics beside the audio, and only if asked.**
A persisted setting, **off by default**, the fourth of this kind and the
companion to the one below.

**1,624 single-passage files; the 702 passages inside captures are skipped on
purpose.** A client tries the sidecar *before* its cache, so one written beside
a capture would overrule the per-song words `[REQ-VIS-215]` puts there and show
all twelve songs at once for every one of them. Skipping captures is what keeps
the two settings complementary rather than one undoing the other
`[SPEC-LYR-080]`.

**This is the route that can reach another machine — on one condition.** A
client builds this path from its own music-folder setting, so it works where
that client can read the music folder and not otherwise `[SPEC-LYR-085]`. A file
already there is never replaced, and a second run writes nothing.

**`[REQ-VIS-215]` Vaino may write per-song lyrics into a local client's cache,
and only if asked.** A persisted setting, **off by default**, the third of this
kind.

A sidecar belongs to a file, and a capture is one file holding a dozen songs —
so `<audiofile>.lyrics` can only ever show all twelve at once. A client's own
cache is keyed by **artist and title**, which a cue track has `[SPEC-MPD-056]`,
so writing there gives every passage its own words. 2,235 songs on this library.

**Two things make this a heavier ask than the other two, and both are said on
the settings page rather than only here.** It writes into another application's
data folder, not the listener's music folder. And **the cache is on the machine
the client runs on**: it works when Vaino and the client share a machine, and
does nothing at all when the client is a phone in another room `[SPEC-LYR-075]`.

**A file already there is never replaced**, whatever it holds — a client may
have fetched and saved it, and that was its choice about its own cache. Written
once, a second run reports every song as already current and touches nothing.

**`[REQ-VIS-210]` Vaino may write cover art into the music folder, and only if
asked.** A persisted setting, **off by default**, beside the cue one.

MPD's art is directory-based: a picture embedded in the file, or `cover.jpg` in
the song's folder. **None of the 191 captures here carry embedded art**, so a
guest falls back to its own artist-level lookup and every album by an artist
wears one cover. Vaino already holds the right pictures `[REQ-VIS-170]`; this
puts one where MPD looks.

**Only where the capture is alone in its folder.** A folder has room for exactly
one cover, and 77 captures share theirs with other captures — six in `Various`,
five in `Eagles`. Writing there would give several albums the same picture,
which is the symptom rather than the cure, so those are skipped **and counted**:
a listener is told how many were left and why, not given a number that looks
like a failure.

**A folder that already has a cover is never touched**, under any of the names
MPD looks for and whoever wrote it. A picture already there was somebody's
choice. That also makes the run idempotent: written once, the folder has a cover
and is skipped thereafter, including over Vaino's own.

**`[REQ-VIS-205]` Vaino may write cue sheets into the music folder, and only if
asked.** A persisted setting, **off by default**, on the settings page.

A whole-side capture holds one set of tags, so a guest names every passage
inside it after the file — 34.1% of this library `[SPEC-MPD-052]`. A `.cue`
sheet beside the capture fixes that for every client, because MPD exposes a cue
track as its own song with its own title `[SPEC-MPD-056]`.

**The setting exists because of where the files go.** Nothing else in Vaino
writes into the listener's music folder, and a player that quietly starts doing
so has taken a decision that was not its to take. So it is asked for, it says
what it will do before doing it, and it is off until then.

Three properties the implementation must keep: it is **idempotent** (a sheet
already matching is left alone, so the folder is not rewritten on a whim); it
**never overwrites a sheet Vaino did not write**, since one that was already
there may be why the library is arranged as it is; and **unticking leaves
written sheets alone**, because deleting files from someone's music folder is a
larger act than declining to add more, and is not what unticking a box asked
for.

**`[REQ-VIS-200]` A running server must say which build it is.** The crate
version and the commit it was built from, stamped in at compile time and
published to every skin so any of them can show it.

The question is asked most often when something expected is **missing** — a
control that is not there, a fix that seems not to have landed, an appliance
deployed to twice — which is exactly when "which build am I looking at" is
hardest to answer by any other means, and when asking a person to remember is
least likely to work.

**A tree with uncommitted changes reports `+dirty`.** A hash alone says which
commit the tree was *at*, not what was compiled, and a build from an edited tree
is not that commit. Reporting it as one would be a confident wrong answer of the
kind `[PI3-API-030]` exists to refuse. Absent git is not a failure: the version
stands and the hash reads `unknown`.

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

**`[REQ-LIB-146]` Read a file's tags from wherever the container actually put them, not wherever the majority format happens to.** `ingest_folder.py`'s `probe()` asked `ffprobe` only for `format`-level tags. MP3's ID3 tags live there, so 5,490 of 5,682 `.mp3` files in the library read fine — but Ogg Vorbis comments land on the *stream* instead, and `format_tags` never sees them: **all 27 `.ogg` files in the library came back with `title`/`artist`/`album`/`track_no`/`disc_no` entirely NULL**, despite every one of them carrying a full tag set on disk.

> Found live 2026-08-31 chasing why `tools/suggest_release.py` scored 0/14 tracks matched for `Xavier Rudd/White Moth` even after being pointed at the exact right release directly by id — the release was right; the folder's own files had nothing in `file_tags` to match titles against at all.
>
> Fixed by asking `ffprobe` for `stream_tags` as well as `format_tags` in the same call, and falling back to the stream-level value per field only when the format level has nothing — a format-level tag a file genuinely has is never overwritten by a same-named stream-level one, verified by a test constructed the other way round for exactly that reason.
>
> Backfilled by `tools/backfill_file_tags.py`, scoped to a `file_tags` row that is entirely empty (the specific shape a probe that looked in the wrong place for every field produces, not "missing one optional field", which can be genuinely true of a file's real tags) — idempotent, and reports rather than retries a file that turns out to have no tags to find at all.

**`[REQ-LIB-150]`** Relocate a moved or renamed library by content, not path `[SPEC-SC-035]`.

**`[REQ-LIB-170]` Sampo has an interface, and it shows what is known, what is not, and what it decided.** The pipeline is seven stages `[SPEC-SA-020]` whose order is recorded nowhere, `ingest_decisions` is written and read by nothing, and a folder of new music is discovered only when a person thinks to point a tool at it — a four-track EP went four months unnoticed. That is `[GDE-BMK-050]`'s undocumented ritual with better parts, and `[REQ-LIB-100]` is not met by a pipeline that works only when someone remembers it exists. Designed in [SPEC013](SPEC013-sampo-console.md).

**`[REQ-LIB-175]` A passage's boundaries, fades and gain are editable while hearing the edit.** *(Requested 2026-08-27; built 2026-08-27.)* Automatic segmentation and amplitude analysis are estimates, not guarantees, and `[REQ-VIS-130]` already requires them reviewable and overridable through a waveform view. What was missing was the concrete shape of that view — start, end, lead-in, lead-out and gain, each draggable, each auditioned by playing the change back before it is kept. Designed in [SPEC021](SPEC021-waveform-boundary-editor.md); built in Vaino and reached from Sampo's profile page, per `[SPEC-SUI-135]`.

**`[REQ-LIB-180]` A recording, artist, release or track id can be corrected by searching MusicBrainz directly, not only by choosing among fingerprint-suggested candidates.** *(Requested 2026-08-27; built 2026-08-27, artist and recording only — release and track search remain designed, not built, per [SPEC010 §3](SPEC010-identification-review.md#3-searching-musicbrainz-directly).)* `[REQ-LIB-165]`'s review queue settles the common case — AcoustID suggests the right answer and a person confirms it — but has no path for the others: self-released audio, a remaster AcoustID has never indexed, or a *credit* that is simply wrong (the right recording, filed under the wrong performer) while the recording id itself is fine. A person must be able to open the candidate's own MusicBrainz page to see what an id actually names before trusting it, and to search MusicBrainz by name when no suggested candidate is right. Designed in [SPEC010 §3](SPEC010-identification-review.md#3-searching-musicbrainz-directly).

**`[REQ-LIB-185]` An edit applied on one installation can reach another that already holds the same music, without a full library replacement.** *(Requested 2026-08-27; built 2026-08-27.)* The bundle transport (`[SPEC-SUI-095]`) carries new audio and its derived facts, but a receiver that already holds an `audio_md5` treats it as fully present and applies nothing further to it — correct for new music, useless for a correction to a track both installations already have. Designed in [SPEC006 §9](SPEC006-data-flow-and-portability.md#9-syncing-an-applied-edit-to-a-remote-installation).

**`[REQ-VIS-265]` A track can be flagged "for review" from the play-history page, on or off at any time.** *(Requested 2026-08-27; built 2026-08-27.)* Hearing something wrong — a misidentified track, a boundary that clips a note, a credit that names the wrong performer — happens while listening, not while inducting, and there was no way to mark it for later without leaving the player. One checkbox per row, keyed by recording where the play had one and by passage where it did not, since an unidentified track is exactly the case most worth flagging. A plain toggle, not a decision: nothing is applied, nothing is refused, checking and unchecking are the same call with the state reversed.

**`[REQ-VIS-270]` A passage's own facts are reachable from Vaino's own browse listing, on an appliance with no Sampo at all.** *(Requested 2026-08-31; built 2026-08-31.)* Sampo's profile page already shows this for the desktop; vainopi has none of it. An info link on each browse row opens `/passage/:id`, unconditional like `/browse`/`/why` beside it -- span, lead/gain/fade, boundary source, every recording this passage names with its credited artist(s), and its own `album`/`radio` sibling `[GDE-BMK-030]` if the file has one. Deliberately narrower than Sampo's own page: no decision history, no MusicBrainz release candidates -- nothing that would need this build to reach out over a network `[SPEC-SA-100]`'s own boundary already refuses to cross the other way. A read against the same database `browse`/`why_for` already query, no decoder, no allocation that scales with the library -- the same cost class already running on a Pi Zero 2W today, not a new one.

**`[REQ-VIS-275]` Whether a reviewed edit has reached the library at all is blatantly obvious, not something a person has to already know to check.** *(Requested 2026-08-31; built 2026-08-31.)* Found live: a boundary edit saved in Vaino's own editor read as identical to one already pushed to vainopi, from this very console's profile page -- `boundary_reviews.applied_at IS NULL` was never surfaced anywhere, so the page showed the same thing whether an edit was a live fact or a draft nobody had folded in yet. A warning banner now leads a passage's own profile page, above everything else, whenever `id_reviews`/`boundary_reviews`/`artist_reviews` carries an unapplied row for it, naming the exact command (`tools/apply_reviews.py` / `tools/apply_boundary_reviews.py --commit`) that closes the gap -- not a button: both tools' own long-standing posture is "an edit changes what a passage *is*, and the library is Sampo's to write, not a web click's," and this does not relitigate that. A second, global count (`GET /api/pending`) rides in every page's own header via `console.js`, since every page already loads it -- the one place a badge actually reaches all of them, not just the page someone happened to already be on.

**`[REQ-LIB-190]` Sampo lists what has been flagged, and lets a person choose one to review.** *(Requested 2026-08-27; built 2026-08-27.)* `[REQ-VIS-265]`'s checkbox is set in Vaino; this is where it is worked from. Read-only, like every other view in the console — flagging and unflagging stay Vaino's, since it is listener state `[SPEC-SC-020]` and listener state is Vaino's to write. Choosing a flagged track opens its profile page, which now offers both handoffs `[SPEC-SUI-135]`, `[SPEC-SUI-140]` name — id review and the waveform editor, the second of which had been designed but never actually linked from the page until this closed the gap.

**`[REQ-LIB-195]` A track flagged on one installation can be reviewed on another that shares an overlapping library, and the resulting correction can return to where the flag was made.** *(Requested 2026-08-27; built 2026-08-27.)* `[REQ-VIS-265]`'s flag is set from vainopi's own play-history page — the appliance a listener actually hears something wrong on — but the appliance carries no Sampo to act on it, and `[REQ-LIB-190]`'s list only ever reads a console's own local `listener_flags`, blind to what a *different* installation flagged. `[REQ-LIB-185]`'s sync answers "a correction reaches a remote installation" once the desktop already knows which track; this closes the leg before it — the name of the track — and, once reviewed, carries the correction back by the same mechanism, rather than requiring a whole-library replacement `[PI005 §1]` already found the wrong tool for exactly this appliance. Designed in [SPEC006 §10](SPEC006-data-flow-and-portability.md#10-syncing-a-flags-fate-to-a-remote-installation).

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
> The near-miss worth naming: occasion weighting `[SPEC-DIR-130]` is seasonal, computed from month and day against the system clock `[SPEC003 §3.3]`. It is *not* a calendar integration and must not become one — reading real appointments would make selection fail when a remote service is unreachable.
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
