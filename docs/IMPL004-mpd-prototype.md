# IMPL004: Prototyping the MPD Director

**Implementation Guide — the build order for [SPEC015](spec/SPEC015-mpd-director.md), riskiest part first**

> **Related:** [SPEC015](spec/SPEC015-mpd-director.md) is the design · [GUIDE007](GUIDE007-external-backends-investigation.md) measured the cost · [IMPL003](IMPL003-sampo-console-build.md) for the same shape of plan

Six stages on `feat/mpd-director`. Each ends in a **measurable claim**, and the order is chosen so that the assumption most likely to be wrong is tested before anything is built on it.

---

## 0. Two things that are true before starting

**`[IMPL-MPD-005]` `vaino-core` is not needed for this.** GUIDE004 wants that extraction for phones, where a separate crate must cross a language boundary `[GDE-AND-045]`. `vaino-mpd` is a binary in *this* crate: it calls `Library::director()` directly, exactly as `vaino` and `station` do. **The port that needs the extraction is not this one**, and conflating them would put a refactor in front of a prototype.

**`[IMPL-MPD-008]` There is no MPD to test against yet, and that is the first decision.** Two candidates, and they test different things:

| Where | Tests | Cost |
| :--- | :--- | :--- |
| **On the appliance**, pointed at `/srv/library/audio` | the **same-tree** case, on the real library, with correct Linux paths after relink | changes the appliance; MPD would index 7,230 files |
| **A container on the desktop**, pointed at the Windows `Music` tree | the **cross-platform** case, and therefore `[SPEC-RLK-025]`'s private-use codepoints | none, but a different filesystem to the one that matters |

**The container is the better first target** — it costs nothing, it is repeatable, and it exercises the mapping's hard case rather than its easy one. The appliance comes later, and only with a decision to install MPD there.

> **A logistics note.** The appliance is currently answering ping intermittently and refusing ssh while playing over Bluetooth — one antenna serving both radios, which `[PI3-FOUND-010]` already measured. Testing a network protocol on that machine *while it plays* is testing two things at once.

---

## 1. Stage 0 — the mapping, read-only

**`[IMPL-MPD-010]` Nothing else starts first.** Both SPEC015 and GUIDE007 say so, and the reason is that a clean trait over an unreliable mapping is a well-organised way to play the wrong song `[SPEC-MPD-060]`.

Build a tool that reads `vaino.db` and an MPD instance and **writes nothing to either**: for every row in `files`, attempt the resolution ladder — same-tree prefix, then `MUSICBRAINZ_TRACKID`, then unresolved — and report the counts.

> **Claims.** A number for each rung, over all 5,709 files. Specifically: **how many of the 276 private-use paths (4.8%) survive a prefix match against a Linux MPD** — the answer is expected to be *none*, and measuring that is the point. Ambiguity is reported separately from failure: two Vaino rows resolving to one URI is a different problem from a row resolving to nothing.

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
