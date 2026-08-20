# IMPL003: Building the Sampo Console

**Implementation Guide — the build order for [SPEC013](spec/SPEC013-sampo-console.md), and what each stage must prove**

> **Related:** [SPEC013](spec/SPEC013-sampo-console.md) · [SPEC006](spec/SPEC006-data-flow-and-portability.md) · [SPEC012](spec/SPEC012-library-relink.md) · [GUIDE002 §3](GUIDE002-rearchitecture-plan.md#3-phased-plan) — this is `[GDE-PHS-040]`'s ingest half, told as a work order.

Nothing in SPEC013 is built. Six stages, ordered by what unblocks the most and what fails soonest if the design is wrong. Each ends in a **measurable claim**, not a demo `[GDE-LES-030]`.

---

## Ordering, and why it is this one

**`[IMPL-SUI-010]` The riskiest thing in SPEC013 is not the console — it is the payload.** `[SPEC-SUI-130]` puts two implementations, in two languages under two licences, on either end of a format that does not exist yet. Nothing keeps them in agreement, and the failure mode is a bundle that imports cleanly and means something slightly different than it said. So the format and its fixtures come **before either half is written**, not between them.

Everything else follows cost and safety: read-only views are the cheapest useful thing and cannot damage a live library, so they come early; anything that writes waits for a job model that assumes a player is writing too `[SPEC-SUI-082]`.

| stage | depends on | why here |
| :--- | :--- | :--- |
| 0 · induct Frisina by hand | — | clears real backlog, and produces the reference transcript |
| 1 · payload schema + fixtures | — | `[IMPL-SUI-010]`; runs in parallel with 0 and 2 |
| 2 · read-only views | — | safe against the live db; answers the question that started this |
| 3 · jobs and induct | 0, 2 | first writes; 0 gives it something to be checked against |
| 4 · bundle, both halves | 1, 3 | export is a job; the format must already be pinned |
| 5 · handoff and launch | 2 | affordances live on the profile page |

Critical path to *"the new music plays on the appliance"* is **0 → 1 → 3 → 4**.

---

## Stage 0 — Induct the pending music by hand

**`[IMPL-SUI-020]` Do not hold the pending tracks hostage to a UI project.** `Frisina, Gerardo` — two albums, **4 audio files** and 2 `cover.jpg`, 40 MB in total — is inductable today with the CLIs exactly as they stand, and the console is months of work away. Run the pipeline by hand, on the real folder, and **keep every stage's output**.

> **Six files, four tracks.** The transfer unit and the induct unit are not the same number and this document originally used one for both. rsync moves 6 files; `ingest_folder.py` inducts the 4 that are audio. Anywhere a count appears, it now says which.

```
python tools/ingest_folder.py data/vaino_new.db "C:/Users/Mango Cat/Music/Frisina, Gerardo"
python tools/ingest_folder.py data/vaino_new.db "C:/Users/Mango Cat/Music/Frisina, Gerardo" --commit
python tools/extract_library.py data/vaino_new.db
python tools/fingerprint_ids.py data/vaino_new.db  ;  ... --merge
```

Cover art needs nothing: both albums already carry `cover.jpg` beside the tracks, which is where [tags.rs](../player/src/tags.rs) looks.

> **This is not just backlog.** The transcript *is* the specification for what `[SPEC-SUI-085]`'s progress display must render — real stage names, real timings, real failure text on a real folder. Designing that view against imagined output is how it ends up showing a spinner and a percentage that means nothing.

> **DONE 2026-08-20.** All four inducted into the live `data/vaino_new.db`, every claim met.
>
> | | |
> | :--- | ---: |
> | files / passages / recordings | **5,705 → 5,709**, +4 each |
> | flavor, per track | **71 / 71** characteristics |
> | extraction | 4 of 4, **0 failed**, 42 s at 19 jobs |
> | `listener_play_history` | 37,238 → **37,238**, unchanged |
>
> A listener-state backup was taken first — 2.4 MB, on `backup_now`'s own stated grounds, *"before letting a tool loose on the library"*. The player then took another at startup by itself, which is `[REQ-LIB-160]` working unprompted.
>
> Browse resolves them: **Gerardo Frisina, 4 passages**, with titles, albums and track numbers from the file tags, and `POST /queue/…/last` returns 204 — the player reads and enqueues them. Export to the appliance **was not attempted**; it waits for stage 4.

**`[IMPL-SUI-025]` What stage 0 found, which is the reason it goes first.**

1. **All four came back `unmatched` from AcoustID**, so they keep their `local:audio:` ids. Predicted exactly by `[SPEC-SUI-075]`, and `unmatched` is not a finding `[REQ-LIB-165]` — but it means the four will surface in the review queue under `no-mbid`, and the console must present that as *"no MusicBrainz entry exists"* rather than as a defect awaiting repair.
2. **The fingerprint pass had 140 passages outstanding, not 4.** 8,190 of 8,330 were checked; the backlog was invisible because nothing reports it. That is `[SPEC-SUI-040]`'s job — a library-wide view of which stages have run over what — and it is now a demonstrated need rather than an inferred one. (Results: 133 confirmed, 7 unmatched, **0 contradicted**.)
3. **Stage output is not display-ready.** `ingest_folder.py` renders titles as `�Duala�` on a Windows console — its `say()` fallback mangling the smart quotes. Harmless in a terminal, wrong in a browser, and proof that `[SPEC-SUI-085]` must render from **structured** stage results rather than by piping stdout into a page.

---

## Stage 1 — Pin the payload, and give both halves the same fixtures

**`[IMPL-SUI-030]` `[SPEC-DF-065]` promises one payload schema and does not contain one.** It says "one serializer, one parser, one schema version" and then describes three envelopes. That was sufficient while nothing implemented it. `[SPEC-SUI-130]` is what makes the absence expensive.

Deliverables, in order:

1. **SPEC014 — the payload schema.** Fields, types, units, and — the part `[SPEC-SUI-165]` cannot work without — the **required set**. "Compatible" is defined as *the receiver can construct what it requires*, so that requirement list is a normative artifact, not documentation.
2. **A fixture corpus** both implementations test against: valid payloads, a newer payload carrying unknown fields, one missing a required field, one with an unresolvable conflict, and the **same bundle twice** `[SPEC-SUI-180]`. Each with its expected outcome.
3. **Settle `[SPEC-SUI-180]` here**, not in stage 4. A resend after a dropped connection over an 11-hour link is the ordinary case, not a mistake, and idempotency designed after the writer exists is idempotency retrofitted.

**`[IMPL-SUI-035]` Keep `[SPEC-SUI-175]` deferred but not foreclosed.** The re-read trigger is genuinely open; storing the payload **with its declared version** costs nothing now and is the whole prerequisite. Decide the trigger later; do not make it impossible today.

> **DONE 2026-08-20.** [SPEC014](spec/SPEC014-payload-schema.md) written; [`tools/payload.py`](../tools/payload.py) is the one serializer; eight fixtures registered with expected outcomes in [`fixtures/payload/`](../fixtures/payload/README.md). `compatible()` mechanises **both** halves of `[SPEC-SUI-165]` — a checker with only the required-set half passes fixture 04. Fixture 01 is generated from the **real library**, not hand-written, so the format met data before anything was built on it. `serde`/`serde_json` are already in [player/Cargo.toml](../player/Cargo.toml), so the importer needs no new dependency.

**`[IMPL-SUI-037]` What stage 1 found.**

1. **The size estimate in `[SPEC-DF-093]` is out by ~9×.** Readable JSON is 11.0 KB per track, not the "~1–2 KB" it argues from; 1.27 KB is the *gzipped* figure `[SPEC-PL-090]`. **The decision it was defending survives** — compression reaches the stated size while keeping inspectability — but the arithmetic under it did not, and `[SPEC-SUI-165]`'s "~16 MB" was my own invention, now measured at 10.4 MB.
2. **73% of the 1,072 MB library is cache no receiver can use** — `musicbrainz_cache` 547 MB, `lowlevel_cache` 202 MB, `identification_cache` 37 MB `[SPEC-PL-040]`. A far stronger form of `[SPEC-SUI-095]`'s argument than the size ratio that motivated it.
3. **The scoped relink of `[SPEC-SUI-105]` is not a separate pass.** Relink never creates a row `[SPEC-RLK-090]`, so it cannot bind an arriving one; the importer must hash and bind what it creates, which is the same walk `[SPEC-PL-085]`. Stage 4 is smaller than planned.
4. **The reference tracks have no `recording_artists` rows at all.** Their names live only in `file_tags`, so tags had to join the payload or the music would land artist-less `[SPEC-PL-050]`. Found by generating from real data; a hand-written fixture would have had artists in it.

---

## Stage 2 — The read-only console

**`[IMPL-SUI-040]` Views first, because they cannot break anything.** The library is WAL `[SPEC-SUI-082]`, so readers never block the player. A console that only reads can be run against the live database on day one, which is the fastest route to finding out whether the design is right.

Build: the server shell `[SPEC-SUI-010]`, `/library` with the profile page `[SPEC-SUI-040]`, `/folder` with the cheap pass `[SPEC-SUI-060]`.

Leave out, deliberately: every POST. No jobs, no induct, no export.

> **Claims, against ground truth measured 2026-08-20.** The folder view on the real Music root reports **7,232** audio and asset files against **5,709** library rows *(5,705 before stage 0)*. Frisina is no longer `unknown`; what remains is the never-indexed tail relink already counted — **28** files, including a scratch directory, less the 4 stage 0 took — and re-measuring it exactly is part of this stage rather than an input to it `[SPEC-RLK-070]`. A file passed on size and mtime is labelled *assumed*; one that was hashed is labelled *verified*; the two are never the same word.

---

## Stage 3 — Jobs, and induct

**`[IMPL-SUI-050]` The job model is the real work here; induct is a thin caller.** `[SPEC-SUI-082]` is the constraint that shapes it, and the pattern is already in the tree rather than ours to invent: a long pass opens the library **read-only**, writes to a sidecar, and folds in with `--merge` when things are quiet. Copy it.

Then `[SPEC-SUI-070]`'s propose-then-commit, `[SPEC-SUI-085]`'s database-held job state, and SSE progress rendered from stage-0's real output.

> **Claims, and this one is unusually clean:** re-running stage 0's Frisina induction *through the console* changes **nothing** — `files.audio_md5` is `UNIQUE`, so a second ingest is a no-op, and the console and the CLI must agree to the row. A job killed mid-pass loses at most the in-flight item `[SPEC-SA-028]`, and the player's writes are never blocked for longer than one stage's lock.

---

## Stage 4 — The bundle, both halves

**`[IMPL-SUI-060]` Exporter and importer are one feature and land together.** Sampo's exporter is Python and AGPL; Vaino's importer is Rust and MIT, and must be written as Vaino code `[GDE-ARC-018]`. Both are tested against stage 1's fixtures **before** either meets the network.

Order: exporter → importer → scoped relink `[SPEC-SUI-105]` → the real transfer.

> **Claims, measured on the appliance:** the Frisina bundle carries 40 MB of audio and single-digit KB of payload, against ~1.02 GB to ship the database. **The appliance's `listener_play_history` row count is identical before and after the import** — the class-D proof, and the only one that matters `[SPEC-SUI-100]`. Scoped relink reports *"verified 6 of 7,238; the remainder were not examined"* and never the word `matched` alone `[SPEC-RLK-140]`. An incompatible fixture leaves the target byte-identical `[SPEC-SUI-165]`.

---

## Stage 5 — Handoff and launch

**`[IMPL-SUI-070]` Small, and last because it is small.** `[SPEC-SUI-155]`'s embed-or-link, `[SPEC-SUI-170]`'s launch sequence.

> **Claims:** with no player running, taking a handoff starts one **on the console's own database path** and the passage plays. With a player already running, no second process appears. With the binary renamed so it cannot start, the operator is told which capability is unavailable and why — not shown a dead link `[SPEC-DF-095]`.

---

## Not on this path

**`[IMPL-SUI-080]` The waveform editor is Vaino's, and it is `[GDE-PHS-040]`'s third deliverable, not this one's.** `[SPEC-SUI-135]` settles *where* it lives; `[SPEC-SA-080]` says what it must do; neither is a reason to build it before the console can reach it. It wants its own requirements and specification first, exactly as segmentation does.

**`[IMPL-SUI-085]` Segmentation stays provisional** `[SPEC-SA-070]`. Stage 0 works because single-track files need no segmentation; a DAO capture arriving before `[GDE-PHS-040]` lands still needs hand work, and the console must say so rather than produce one whole-file passage and call it done `[SPEC-SUI-075]`.

---

**Traceability:** `[IMPL-SUI-010..085]` · implements `[SPEC-SUI-010..180]` · sits under `[GDE-PHS-040]`
