# IMPL004: Prototyping the MPD Director

**Implementation Guide — the build order for [SPEC015](spec/SPEC015-mpd-director.md), riskiest part first**

> **Related:** [SPEC015](spec/SPEC015-mpd-director.md) is the design · [GUIDE007](GUIDE007-external-backends-investigation.md) measured the cost · [IMPL003](IMPL003-sampo-console-build.md) for the same shape of plan

Six stages on `feat/mpd-director`. Each ends in a **measurable claim**, and the order is chosen so that the assumption most likely to be wrong is tested before anything is built on it.

---

## 0. Two things that are true before starting

**`[IMPL-MPD-005]` `vaino-core` is not needed for this.** GUIDE004 wants that extraction for phones, where a separate crate must cross a language boundary `[GDE-AND-045]`. `vaino-mpd` is a binary in *this* crate: it calls `Library::director()` directly, exactly as `vaino` and `station` do. **The port that needs the extraction is not this one**, and conflating them would put a refactor in front of a prototype.

**`[IMPL-MPD-008]` There is no MPD to test against yet, and that is the first decision.** Three candidates, not two, and they are not alternatives so much as tests of different things.

| | tests | audio | risk |
| :--- | :--- | :--- | :--- |
| **MPD on Windows**, native, at the real `Music` tree | the **same-tree prefix** rung, on paths identical to `vaino.db`'s | real | none |
| **MPD in a Linux container** | the **cross-platform** rung — what happens when 276 private-use paths meet a filesystem with real colons `[SPEC-RLK-025]` | awkward | none |
| **MPD on the appliance** | the real deployment | real | **see below** |

Current Windows builds exist and are not an afterthought: **0.24.14, 13 August 2026** — the same day as the Linux release.

**`[IMPL-MPD-009]` The appliance is not an additional test bed. It is a conversion.** This is the consideration that decides the order, and the first draft of this plan missed it.

In the MPD topology Vaino has **no audio path** — MPD owns the device `[SPEC-MPD-030]`. The appliance's sink is a single Bluetooth speaker, and two players cannot hold it. So running MPD there means **stopping `vaino`**, and the appliance stops being a Vaino-plays box and becomes an MPD-plays box for the duration.

That is a decision about what the appliance *is*, not a test setup. It is also the thing eventually worth proving — a moOde or Volumio owner is in exactly that configuration `[GDE-BAK-080]` — but it should be proved once the design works, not while it is being discovered.

**`[IMPL-MPD-010a]` So: Windows and container for stage 0, Windows through stage 2, the appliance only on purpose.**

Stage 0 needs **no audio at all** — it reads `vaino.db` and an MPD database and writes nothing `[IMPL-MPD-010]` — so the container's weakest point does not apply to it, and running both costs little more than running one. They answer different questions and both answers are wanted.

> **One cost to measure rather than assume.** MPD indexing 7,232 files across a Windows bind mount into a container may be slow enough to matter. If it is, the cross-platform rung can be tested against a **synthetic tree** carrying the real Linux names and no audio — which proves name matching exactly, and gives up only the tag-based rung.

---

## 1. Stage 0 — the mapping, read-only

**`[IMPL-MPD-010]` Nothing else starts first.** Both SPEC015 and GUIDE007 say so, and the reason is that a clean trait over an unreliable mapping is a well-organised way to play the wrong song `[SPEC-MPD-060]`.

Build a tool that reads `vaino.db` and an MPD instance and **writes nothing to either**: for every row in `files`, attempt the resolution ladder — same-tree prefix, then `MUSICBRAINZ_TRACKID`, then unresolved — and report the counts.

> **Claims.** A number for each rung, over all 5,709 files. Specifically: **how many of the 276 private-use paths (4.8%) survive a prefix match against a Linux MPD** — the answer is expected to be *none*, and measuring that is the point. Ambiguity is reported separately from failure: two Vaino rows resolving to one URI is a different problem from a row resolving to nothing.

> **DONE 2026-08-21, Windows, MPD 0.24.14.** Native build against the real
> `Music` tree; MPD indexed **5,758 songs** against the library's 5,709 rows.
> [`player/src/mpd.rs`](../player/src/mpd.rs) is the protocol client —
> `std::net` and nothing else, as `[SPEC-MPD-070]` claimed — and
> [`mpd_map`](../player/src/bin/mpd_map.rs) is the measurement.
>
> | rung | rows | |
> | :--- | ---: | ---: |
> | 1 · same-tree prefix | **5,709** | **100.0%** |
> | 2 · recording MBID | 0 | 0.0% |
> | 3 · unresolved | **0** | 0.0% |
>
> **Ambiguous: 0.** No library row resolves to more than one MPD song.
>
> **All 276 private-use paths resolved.** MPD on Windows reports the *same*
> `U+F03A` substitution Vaino stored, so the two agree without either knowing
> why. That is the same-platform case behaving well; it says nothing yet about
> a Linux MPD, which is the case that will not `[IMPL-MPD-008]`.

**`[IMPL-MPD-012]` The 284 shared MBIDs are not an ambiguity. They are the model
working, and Vaino already implements it.** An earlier reading of this
measurement called them a hazard — the right music in the wrong file — and that
was wrong in both directions.

**Selection is recording-scoped; playback is encoding-scoped.** Two rips of one
recording share an artist, a recording id and a flavor vector, so for rotation,
recovery, restraint and artist blocking they are **one thing**. Which of them
sounds is a lower question, and once it is answered *that file's own*
`start_ms`, `end_ms` and gain govern — because those are properties of the
encoding `[SPEC-DF-040]` and of nothing else.

Confirmed in the code rather than assumed: `last_played` and `artist_of` are
keyed by **mbid**, while `length_bonus` is computed from `c.length_s` on each
candidate **row**, and rows are passages. Recording-scope above, encoding-scope
below, already built.

**Which answers how the encoding gets chosen: it is not a separate step.** The
duplicates enter the roulette as their own candidates and compete, and because
the length bonus reads the encoding's own length, a longer or shorter rip is
weighted on its own terms. Rotation then suppresses the *recording*, so
whichever won, the other is suppressed with it and neither returns early.

**`[IMPL-MPD-013]` One real constraint survives, and it is narrower than the
worry it replaces: the times must come from whatever actually plays.** A mapping
may resolve to any encoding of the right recording. What it may **not** do is
hand MPD a URI for file B while sending `rangeid` computed from file A's
passage — that would apply one rip's trim to another's audio, which is the
encoding-scope rule broken rather than applied.

So the mapping's output is a **pair from one file** — `(uri, passage)` — never a
URI from one and times from another. Where rung 1 resolves, that is automatic.
Where rung 2 resolves by recording, the passage must be re-chosen to match the
song MPD will actually play, and if Vaino holds no passage for that encoding it
has nothing legitimate to send and should say so.

**`[IMPL-MPD-014]` MPD knows 49 songs the library does not.** 5,758 against
5,709, and relink counted a comparable never-indexed tail `[SPEC-RLK-070]`.
Those are ingest's business, not the mapping's `[SPEC-RLK-090]` — but a
`vaino-mpd` that silently ignored them would be hiding a list someone might want.

**`[IMPL-MPD-015]` If the ladder resolves poorly, stop and redesign.** A third rung — `(artist, title, duration)` within a tolerance — is the obvious next idea and is exactly the kind of fuzzy key that produces a plausible wrong answer. It should not be added on instinct; it should be added only if stage 0 shows the first two rungs are insufficient, and then measured for false matches rather than coverage.

---

## 2. Stage 1 — observe, write nothing

**`[IMPL-MPD-020]` A client that connects, watches, and judges.** `idle player playlist options`, `status`, `currentsong`, and the elapsed sampling `[SPEC-MPD-090]` requires — at the configured interval, default 5 s `[SPEC-MPD-105]`.

It reports what it *would* record. It does not write `listener_play_history`, does not touch the queue, and does not set stickers.

> **Claims.** Play tracks by hand from any MPD client and the observer agrees with the scrobbling rule: a track played past half its length (or four minutes) is a play, one skipped earlier is not, and a **12-second passage played whole is a play** — which is where the dropped anti-spam floor is proved rather than asserted `[SPEC-MPD-090]`.

---

## 3. Stage 2 — enqueue, without the Director

**`[IMPL-MPD-030]` Prove `rangeid` before proving anything about selection.** This is the single most load-bearing assumption in SPEC015: it is why MPD is the target and OpenSubsonic is not `[GDE-BAK-035]`. Test it with a **trivial random selector**, so a failure here is a protocol failure and not a Director one.

`consume 1`, `addid`, `rangeid <id> <start:end>`, top up to depth `[SPEC-MPD-105]`.

> **Claims.** A passage plays **its span** and stops — verified against a DAO capture, where naming the file would otherwise play forty songs. The queue holds at depth and refills as `consume` drains it. And the etiquette holds under a person's hands `[SPEC-MPD-095]`: adding twenty tracks stops it adding, clearing refills to five, removing a pick produces a **different** one, and reordering is left alone.

---

## 4. Stage 3 — the Director drives

**`[IMPL-MPD-040]` Swap the random selector for `Director::decide`.** The exclusion set is the queued passage ids and the flow tail is the last of them `[SPEC-DIR-160]`, which is what `Session::refill` already does — this stage is mostly about *not* reimplementing that.

**`[IMPL-MPD-045]` And the bookkeeping is the part to get right, not the selection.** `note_queued` when a passage is added; `forget_queued` when one leaves without playing `[REQ-PD-112]`. Locally that is driven by `take_dropped`; here it is a queue diff after `idle playlist`, and a deletion by a person must reach the same place as a file that would not open.

> **Claims.** Over a session, the census `[GDE-PD-010]` behaves as it does locally: rotation suppresses what has just played, artists block, and a passage deleted from the queue by hand does **not** stay suppressed.

---

## 5. Stage 4 — write back, and publish

**`[IMPL-MPD-050]` Plays into `listener_play_history`; nothing into anyone's scrobbler** `[SPEC-MPD-100]`.

**`[IMPL-MPD-055]` Stickers under `vaino.`** — `vaino.passage` as the mapping cache, then `vaino.why`, `vaino.flavor`, `vaino.chosen_at` `[SPEC-MPD-050]`.

> **Claims.** The mapping cache measurably shortens a second run. A sticker-aware client displays the explanation with **no change to that client**. And the class-D guarantee holds in the direction that matters here: `listener_play_history` grows, and nothing Vaino writes leaves the machine.

---

## 6. Stage 5 — settings and containment

**`[IMPL-MPD-060]` The two parameters reach the settings page** `[SPEC-MPD-105]`, following `[REQ-VIS-155]`'s pattern — persisted on change, bounds in the snapshot. Queue depth is a **promotion** of today's `--depth` flag and applies to the local engine too.

**`[IMPL-MPD-065]` Then the feature gate, last.** `--features mpd`, `std::net` only, and the check that matters: **the appliance binary is byte-identical to a build from before this branch** `[GDE-BAK-050]`. Gating first would mean five stages developed behind a flag nobody had turned on.

---

## 7. What could stop this

**`[IMPL-MPD-070]`** Named now so they are recognised rather than discovered:

1. **The mapping resolves badly** `[IMPL-MPD-010]`. The most likely failure and the reason it is stage 0.
2. **Two writers, one queue.** A person and a daemon both editing means races the design discusses `[SPEC-MPD-095]` but has not implemented. MPD's `status` carries a **playlist version**; using it to detect a change between read and write is the obvious guard and is untried.
3. **`rangeid` may behave differently than documented** across MPD versions, or with some decoders. Stage 2 exists to find that out early.
4. **The appliance is a poor test bed while it plays** — see the logistics note above.

---

**Traceability:** `[IMPL-MPD-005..070]` · implements `[SPEC-MPD-010..120]` · under `[GDE-BAK-100]`
