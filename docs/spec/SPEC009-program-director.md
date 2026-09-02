# SPEC009: Program Director

**Design Specification — Tier 2**

How Vaino chooses the next passage. Reproduces MuLibPlay's six-years-proven selection algorithm `[GDE-PD-010..050]`, extended with Like/Dislike Taste and the full 71-dimension flavor vector.

> **Related:** [REQ002 §2](REQ002-functional-requirements.md#2-program-director--pd) · [SPEC005 Flavor Distance](SPEC005-flavor-distance.md) · [SPEC008 Schema](SPEC008-database-schema.md) · [SPEC023 Domain Vocabulary](SPEC023-domain-vocabulary.md) — rotation, weighting and flavor below score **recordings** (occasionally passages, where noted); the panel that shows this decomposition is "Why this passage?" `[REQ-VIS-100]`, not "track" · inherited [MCR-SPEC005](../inherited/mcrhythm/MCR-SPEC005-program_director.md), [MCR-SPEC006 Like/Dislike](../inherited/mcrhythm/MCR-SPEC006-like_dislike.md)

---

## 1. The Governing Idea

**`[SPEC-DIR-100]` Two orthogonal mechanisms, kept orthogonal.** This is why MuLibPlay works, and the single most important thing not to break:

| Mechanism | Answers | Inputs |
| :--- | :--- | :--- |
| **Frequency** | *How often may this play?* | rotation, recovery, restraint, play history, occasion |
| **Character** | *Does this fit right now?* | programme seeds, Taste, flavor distance |

They never mix. Frequency produces a weight; character shapes and orders a pool. Conflating them was rejected explicitly `[GDE-OPN-030]`: a taste-match factor folded into the weight product would make "I like this" and "play this often" indistinguishable, and would make the weight undecomposable for `[REQ-VIS-100]`.

**`[SPEC-DIR-105]` Randomness is applied last**, over an already-shaped pool `[GDE-PD-050]`. Character comes from the shaping; surprise comes from the roulette.

---

## 2. Pipeline

```
 all radio passages
   │
   ├─ A. eligibility & frequency ...... weight per passage, or excluded
   ├─ B. pool shaping ................. prune + gather against seeds & Taste
   ├─ C. flow ordering ................ re-sort by similarity to queue tail
   └─ D. weighted roulette ............ rank decay, then weighted random pick
```

---

## 3. Stage A — Eligibility & Frequency

**`[SPEC-DIR-110]` Log-scale time encoding.** `seconds(v) = 10^v × 3600`. One float spans four orders of magnitude: `-1.0` → 6 min, `0.0` → 1 h, `2.0` → 4.2 days, `3.0` → 41 days.

**`[SPEC-DIR-115]` Artist pass, then recording pass**, multiplicatively. For each:

1. `w = 10^(-restraint)`
2. **Hard block** if `now - last_played < seconds(rotation)` → excluded entirely.
3. Otherwise **linear recovery ramp**: `w *= clamp((age - rot) / rec, 0, 1)`.
4. Recording weight multiplies its artist's weight. Related recordings block and damp too, scaled by relation strength.
5. Drop below `min_weight` → excluded.

**`[SPEC-DIR-116]` Related recordings share a rotation, and each is judged on its own age.** A live take, a remaster and the compilation appearance are the same song to a listener; hearing one should suppress the others. MuLibPlay intended this and never achieved it, in two independent ways:

```cpp
QMap<qint32,qreal> relTrk = de.relatedTracks( trackId );
foreach ( qint32 tid, relTrk )                       // iterates VALUES, not keys
  if ( tracksMostRecentPlay.contains( tid ) )        // so tid is a truncated strength
```

Qt's `foreach` over a `QMap` yields values, so `tid` was the relation strength cast to `qint32` — 0 or 1 — and the lookup asked for the play time of track 0. Nothing matched. Even had it matched, the damping call passed `now - tracksMostRecentPlay.value(trackId)`, the *primary* track's age, so a relation would have been judged by the wrong recording's history.

Vaino's rules:

1. **Block** if any related recording played within this recording's rotation window. Strength does not scale the block — sharing a rotation is the point of relating two recordings.
2. **Damp** by each related recording's own ramp, using **its own age**, over a recovery window scaled by relation strength. A weak relation therefore recovers sooner.
3. **Every** relation applies, multiplicatively. Three half-recovered relations yield 0.125, not 0.5.

**`[SPEC-DIR-118]` Master time scales.** One multiplier for artists and one for recordings, over every block and ramp *duration*. Floating point, four decimal places, range 0.0001–100.0000, default 1.0000 — at which they are exactly inert.

They exist because per-subject tuning is log-scale `[SPEC-DIR-110]`. "Everything a bit sooner" is a reasonable thing to want and is otherwise inexpressible without editing thousands of rows. At 0.5 every block and ramp is half as long; at 2.0, twice.

They scale **durations, never weights**. That is what keeps frequency and character orthogonal `[SPEC-DIR-100]`: a scale changes *when* a passage becomes eligible, never *how much* it is wanted, and it remains a single legible term in the panel. The two are independent — a recording scale must not move an artist block — and the recording scale reaches related recordings, which share the recording's windows. Stored in `listener_settings` with the range enforced in the schema as well as in code, since an out-of-range stored value would quietly change selection everywhere.

**`[SPEC-DIR-117]` The artist weight never reaches the recording weight in MuLibPlay as shipped.** Step 4 above describes the intent. The code does something else, and the difference was found only by transcribing it:

```cpp
qreal weight = pow(10.0,-restraint);          // outer
if ( eligibleArtists.contains( artistId ) )
  { weight *= eligibleArtists.value( artistId );   // writes the OUTER weight
    qreal restraint = ...;
    qreal weight = pow(10.0,-restraint);           // SHADOWS it; outer never read again
```

The inner declaration shadows the outer, so the multiplication by the artist weight is dead. `eligibleTracks.insert(trackId, weight)` sits inside that inner block — MuLibPlay's own variable names, quoted rather than translated. **An artist rotation block still excludes the recording — that gate is a map lookup, unaffected — but a partially recovered artist does not damp its recordings at all.** The artist recovery ramp has, in six years of production, done nothing.

**Resolved: Vaino implements the ramp.** MuLibPlay is a proven baseline, not a ceiling — six years of satisfactory listening shows the *design* is sound, which is not the same as showing every behaviour of the binary is worth keeping. Artist recovery exists so that hearing one recording by an artist gently damps the rest until it recovers; that is the intent, and it is better than what ran.

The `ArtistCoupling` enum is named for behaviour rather than provenance, because the choice is now Vaino's:

| | Effect | Purpose |
| :--- | :--- | :--- |
| **`Damped`** (default) | artist ramp multiplies into the recording weight | Vaino's behaviour |
| `GateOnly` | artist can block, never damps | measuring divergence only `[REQ-PD-110]` |

Both are pinned by test so neither can drift into the other. `GateOnly` is retained to *measure* how far Vaino departs from six years of observed behaviour — diagnostic, never a listening mode and never a gate.

**Measured divergence.** Against the migrated library — 8,079 radio passages, 37,134 plays, 428 artists with history of which 329 carry tuned preferences — evaluated at the instant the play history ends:

| | |
| :--- | ---: |
| eligible under `GateOnly` | 2,421 |
| weight changed by the artist ramp | **1,897 (78.4%)** |
| median damping factor | **0.179** (≈5.6× suppression) |
| damping range | 0.0025 – 0.9985 |
| newly excluded below `min_weight` | 69 (2.9%) |

This is not a marginal correction. On four passages in five the corrected ramp changes the weight, and at the median it suppresses a recently-heard artist more than five-fold — which is precisely the artist spacing the mechanism was designed to provide and has never delivered.

Two cautions on the number. It is a **single-instant snapshot at the end of the history**, the most crowded possible moment, so treat 78.4% as an upper region rather than a steady-state rate. And the same shadowing suppressed artist **restraint** as well as the ramp — separated here, restraint independently affects the same 1,897 passages with a median of 1.0023 but a range of 0.0625–4.4978, so the "much more / never again" knob has been inert at the artist level too. Both are restored by the same fix.

Two consequences to watch:

1. **The artist ramp is now load-bearing, so its defaults are too.** Artist rotation 1.0 and recovery 1.0 `[SPEC-DIR-120]` mean an artist blocks for 10 hours and then damps across the following 10. That second window has never had any effect and has therefore never been tuned by anyone. Treat the artist defaults as unvalidated until observed.
2. **Damping can now push a weight under `min_weight`**, excluding a passage early in the artist's recovery where it would previously have been eligible at full weight. This slightly extends the effective block. It is a consequence of the fix, not a separate decision.

**Still open:** the same block contains a second instance of the pattern — related-recording recovery damping passes the *primary* recording's age rather than the related recording's, so it too is largely inert. Related recordings are not yet modelled in Stage A; fix it when they are, rather than porting the defect forward.

**`[SPEC-DIR-120]` Defaults matter more than they look.** Only 2,918 of 8,116 MuLibPlay tracks (36%) ever received tuned values `[GDE-BMK-020]`, so most selection runs on defaults:

| | default | = | observed tuned median |
| :--- | ---: | ---: | ---: |
| recording rotation | 2.0 | 4.2 days | 2.196 (6.5 days) |
| recording recovery | 2.6 | 16.6 days | 2.722 (22 days) |
| artist rotation | 1.0 | 10 hours | 1.231 (17 hours) |
| artist recovery | 1.0 | 10 hours | 1.595 (39 hours) |
| restraint | 0.0 | ×1.0 | 0.000 |

Tuned medians sit close to the defaults, which is evidence the defaults are well chosen — users nudged rather than fought them. Restraint spans −0.939 to 5.0, i.e. an 8.7× boost to a 10⁻⁵ suppression: it is the "much more / never again" knob.

**`[SPEC-DIR-125]` Passage-level filters and length bonus.** `radio` passages only `[REQ-PD-120]`. Reject shorter than 30 s, longer than 3600 s, or starting more than 10800 s into a file. Then `w *= sqrt(min(4.0, 180 s / length))` — a mild preference for ~3-minute passages, capped at 2× for short ones.

**`[SPEC-DIR-130]` Occasion multiplier — a time layer, not a flavor dimension.** User-defined characteristics `[GDE-MCR-060]` supply the *value*; a seasonal curve turns it into a multiplier:

```
w *= 1 + characteristic_value × (curve(today) − 1)
```

So `user.christmas.christmasy = 0.9` on 21 December with a curve value of 4.2 yields ×3.9; the same passage in June yields ≈×1. This keeps MuLibPlay's proven seasonal behaviour `[GDE-PD-020]` while removing the hardcoded `[C]`/`[W]`/`[S]`/`[K]` tags, and — critically — stays **legible as a single term** in the Why-this-passage panel. Folding seasonality into the programme target vector was rejected for exactly that reason.

Curves are data, not code: a new occasion is a new characteristic plus a curve, with no edit to the engine.

**`[SPEC-DIR-132]` Curve representation.** A curve is control points around a **wrapped** year — January follows December — plus an interpolation mode:

| Mode | Behaviour | Use |
| :--- | :--- | :--- |
| `step` | hold the previous point's value | month-granular curves, as MuLibPlay's `[W]`/`[S]`/`[K]` were |
| `linear` | interpolate in **log** space | smooth curves, as `[C]` effectively was |

Interpolation is logarithmic because these are *ratios*: halfway between ×0.5 and ×2.0 is ×1.0, not ×1.25, and a linear blend of 0.000001 and 10 would sit near 5 for half the gap.

Leap years are deliberately ignored — 29 February shares an ordinal with 1 March. A season is not accurate to the day, and honouring it would shift every curve by a day in three years out of four.

The multiplier is clamped at zero. A characteristic value above 1.0 against a curve below 1.0 would otherwise drive it negative and *invert* the weight; "never right now" is the strongest thing a season may say.

**`[SPEC-DIR-134]` The inherited four are data, and the data already exists.** MuLibPlay's `[C]`, `[W]`, `[S]`, `[K]` migrate to `user.christmas`, `user.winter`, `user.summer`, `user.childrens` — already present in the migrated library as **binary characteristics**, `christmasy` paired with `not_christmasy`. The curve attaches to the positive class; the negative class carries no curve and is ignored.

Verified on the migrated library: with the Christmas curve loaded, 81 christmasy recordings (82 radio passages) drop below `min_weight` out of season, and the rest of the pool is untouched. A partial characteristic value damps rather than excludes, which is the point of scaling by value rather than testing a tag.

Note that `[K]` was never seasonal at all — a flat ×0.000001 on 140 children's recordings. It expresses fine as a single-point curve, which is a fair test of whether "curves are data" actually holds.

**`[SPEC-DIR-136]` Loaded 2026-08-13** by `tools/load_occasions.py`, transcribed from `occasionWeight()` in the inherited `musicdirector.cpp`:

| occasion | class | interp | points | peak | reach |
| :--- | :--- | :--- | ---: | :--- | ---: |
| `user.christmas` | `christmasy` | linear | 11 | **×10 on 25 Dec** | 41 passages |
| `user.winter` | `wintry` | step | 6 | ×2 in December | 2 |
| `user.summer` | `summery` | step | 6 | ×2 in June | 1 |
| `user.childrens` | `for_children` | step | 1 | ×0.000001 all year | **149** |

Measured effect in August: eligible passages fall **8,038 → 7,851**, with 187 more dropping under `min_weight` — the christmas and children's passages, suppressed out of season. The mechanism had been complete and inert since it was written; the library already carried the characteristic values from six years of MuLibPlay tagging, so loading the curves is what made that tagging act again.

> **`[SPEC-DIR-137]` The children's weight interacts with `min_weight`, and 0.000001 means *never*, not *rarely*.** A children's passage weighs `average × multiplier`; at the library's average of 1.214 that is 0.0000012, which falls **below `min_weight` (0.001)** and is therefore *excluded entirely* rather than made unlikely. There is a cliff at multiplier ≈ 0.00083.
>
> | kids multiplier | passage weight | eligible? | plays/year @ 60 passages/day |
> | ---: | ---: | :--- | ---: |
> | 0.000001 *(MuLibPlay's)* | 0.0000012 | **no** | **never** |
> | 0.001 | 0.0012 | yes | 0.42 |
> | **0.0024** | 0.0029 | yes | **1.00** |
> | 0.005 | 0.0061 | yes | 2.08 |
>
> **MuLibPlay used the same `kidSongWeight` and the same `minWeightLimit`, so children's songs never played there either.** The stated intent — "extremely rarely, one per year or less" — is satisfied by the *or less*, and the six years of observed behaviour were the *never* end of it. Retained deliberately at 0.000001; `--kids 0.0024` is the value that would make it genuinely once-a-year instead.
>
> The general lesson: an occasion multiplier small enough to look like suppression can cross `min_weight` and become exclusion, and the two are not the same thing — an excluded passage cannot be surfaced by any amount of listening.

> **The children's weight deserves a decision rather than a default.** MuLibPlay's shipped `kidSongWeight` is 0.000001, which is not a de-emphasis but an effective ban, and here it removes **149 radio passages — 1.8% of the library — permanently, in every season**. It is transcribed faithfully because that is what ran for six years, but it is a parameter: `--kids 0.5` merely damps them, and the value is a row rather than a constant.

---

## 4. Stage B — Pool Shaping

**`[SPEC-DIR-140]` Seeds define the target.** A programme is a list of 6–8 exemplar passages `[GDE-PD-040]`, down-selected to at most `max_seeds` — one per artist, least-recently-played. Naming songs beats tuning sliders, and it is the mechanism MuLibPlay users actually exercised.

**`[SPEC-DIR-145]` Prune, then gather** — both over flavor distance `[SPEC-FD-040]`:

1. **Prune** — remove passages *most unlike* every seed until the pool reaches `excl_pool`.
2. **Gather** — take the `rand_pool × 2 / seeds` passages *most like* each seed.

**`[SPEC-DIR-150]` Taste enters here, and only here** `[GDE-OPN-030]`. Like-Taste and Dislike-Taste are weighted centroids of the flavor of liked/disliked songs `[MTA-SMPL-020]`, computed per user.

- **Dislike-Taste acts as an exclusion filter**: passages within `dislike_radius` of it are removed from the pool before gathering. This is McRhythm's own suggested use `[LD-LIKE-021]`, and it is the half that needs no tuning to be useful.
- **Like-Taste acts as an additional seed**, weighted `like_seed_weight` relative to programme seeds.

> **Taste is implemented but unexercised.** `listener_likes` is empty in the migrated library, so both halves have unit tests and no field data behind them. `dislike_radius` and `like_seed_weight` remain **new and unvalidated** `[SPEC-DIR-195]` — there is nothing to tune them against until the listener records a Like.

Rationale: Taste is a *character* signal, so it belongs in the stage that shapes character, leaving frequency untouched `[SPEC-DIR-100]`. Treating a Like as "just another way of naming a song that defines a mood" also keeps it in the same idiom as programmes.

**`[SPEC-DIR-157]` Implemented 2026-08-10, and it is visible in what plays.** The same library at three times of day, six passages each:

| Programme | Queue | Mean length |
| :--- | :--- | ---: |
| **Prog** 19:00 | Steely Dan *Aja*, Traffic, Rush *Jacob's Ladder*, Led Zeppelin, Genesis *Squonk* | ~440 s |
| **Groove** 15:00 | Genesis *Los Endos*, Fatboy Slim, U2, Paula Abdul (dance mix), Massive Attack | ~270 s |
| **Light** 10:00 | Genesis *Invisible Touch*, Beatles *Lovely Rita*, Tom Petty, Heart | ~200 s |

Genesis appears in all three, and a *different* Genesis each time — *Squonk* and *Los Endos* for Prog and Groove, *Invisible Touch* for Light. That is the property the whole metric exists for: similarity by sound rather than by artist. Track length was never an input to shaping; it separates because prog is long and pop is short.

Three implementation decisions worth keeping:

- **"Most unlike every seed" is distance to the *nearest* seed.** A programme is a handful of exemplars, not one centre, so a passage close to any one seed belongs even if far from the rest.
- **Gathering is per seed, not a global top-N.** A global list would let one seed in a dense region supply the whole pool and silently drop the rest of the programme.
- **A passage with no flavor is kept, not dropped.** Unmeasured is not unsuitable; excluding them would make a half-scanned library play only the half it had scanned.

**`[SPEC-DIR-155]` Taste never blocks and never boosts frequency.** A disliked passage is removed from the *pool*; its rotation and restraint are unchanged. If the user later removes the Dislike, behaviour returns exactly to baseline with no residue.

**`[SPEC-DIR-158]` Cold start.** With no history, no Likes and no tuned preferences, Stage A degrades to `10^0 = 1.0` for everything and Stage B has no seeds. Selection then reduces to uniform random over eligible passages — correct, if uninspiring. A programme with seeds is the minimum for interesting behaviour, so first-run setup asks for a handful of exemplars rather than presenting an empty station.

---

## 5. Stages C & D — Flow and Roulette

**`[SPEC-DIR-160]` Flow.** Re-sort the pool by flavor distance to the **last passage already queued**, so consecutive passages blend `[GDE-PD-050]`. This is also what makes a hard programme switch acceptable `[SPEC-DIR-180]`: continuity is supplied here, not by blending programmes.

> **Future direction `[SPEC-FD-170]`:** flow currently matches whole-passage flavor to whole-passage flavor, but a handover is heard as the *end* of one passage against the *start* of the next. Characterising the first and last three minutes separately, and matching `exit → entry`, models the transition rather than the pairing. Not the current target — it roughly triples extraction cost and needs a segment discriminator in `flavor`.

**`[SPEC-DIR-165]` Roulette.** Take the top `rand_pool`, apply rank decay `w *= decay^rank`, then pick weighted-random. Selection is by weight, not by rank — a lower-ranked passage can win, which is where the surprise lives.

**`[SPEC-DIR-167]` Both implemented 2026-08-10, and both measurable.**

*Flow works.* Across eight consecutive selections on the real library, mean distance between a passage and the one it followed was **0.432**, against a library median of 1.052 `[SPEC-FD-085]` — consecutive passages are **59% closer than random pairs**. That is the whole claim of `[SPEC-DIR-160]`, and it is the reason a hard programme switch is tolerable.

*The roulette stays a roulette.* Winning ranks across those eight: **5, 10, 17, 24, 31, 50, 61, 99**. Decay favours the low end without making it certain — one passage won from rank 99 at a roulette weight of 0.006. A director that always returned rank 0 would be evidence of a bug, not of good taste.

Two decisions:

- **Rank decay applies only when there is a flow order.** With nothing queued to follow, rank is whatever order the scan happened to visit, and decaying by it would silently favour the first passage examined. The first pick of a session therefore uses undecayed weights, and the record says so.
- **Runners-up are ranked by *decayed* weight.** "Why not something else?" is a question about what nearly won the roulette, which is the decayed figure, not the frequency weight.

---

## 6. Programme Selection

**`[SPEC-DIR-180]` Hard switch at the programme's start time**, as MuLibPlay does. Eight programmes are defined by start time `[GDE-PD-040]`; the active one is whichever most recently started. Blending was rejected: six years of production show no complaint, the flow stage already smooths transitions `[SPEC-DIR-160]`, and blending introduces a tunable nobody asked for while making "which programme am I in?" ambiguous. Start times are the listener's own local clock, via `listener_settings.utc_offset_minutes` — synced from the OS rather than left at its unconfigured default, since that default is UTC and most listeners are not `[REQ-VIS-255]`.

**`[SPEC-DIR-185]`** Manual programme selection overrides time-of-day until the user reverts to automatic. `POST /program/:id` accepts the literal id `"auto"` as a real, parameterless revert: it clears `manual_program` and lets the time-of-day schedule resume, and the settings panel offers it as an ordinary "Automatic (by time of day)" option alongside the named programmes `[REQ-VIS-255]`.

---

## 7. Visibility Contract

**`[SPEC-DIR-190]`** Every automatic selection writes a `selection_decisions` record `[SPEC-SC-100]` sufficient to reconstruct the choice `[REQ-VIS-100]`: artist weight and block state, recording weight and ramp position, occasion multiplier with the characteristic and curve value that produced it, length bonus, final Stage-A weight, distance to each seed, Taste effect, flow distance, rank, roulette position and target — **and the runners-up that lost, with their weights**.

The orthogonality of `[SPEC-DIR-100]` is what makes this legible: the panel shows *how often* and *does it fit* as two separate stories rather than one opaque product.

---

## 8. Parameters

**`[SPEC-DIR-195]`** Provenance matters here — some values are proven, others are inherited guesses:

| Parameter | MuLibPlay | Status |
| :--- | ---: | :--- |
| `min_weight` | 0.001 | Proven |
| `max_seeds` | 5 | Proven |
| min / max length, max depth | 30 s / 3600 s / 10800 s | Proven |
| length bonus midpoint, cap | 180 s, 4.0 | Proven |
| `excl_pool` | 1000 | **Verified** `[SPEC-DIR-205]` |
| `rand_pool` | 100 | **Verified** |
| rank `decay` | 0.96 | **Verified** |
| `dislike_radius`, `like_seed_weight` | — | **New, unvalidated** |

**`[SPEC-DIR-205]` Re-derived 2026-08-13 — and the answer is to keep them.** Measured by `tools/pool_params.py` over the fully extracted library, comparing where each pool boundary falls in distance terms under both metrics:

| | rank 10 | rank 200 (*gather*) | rank 1000 (*excl_pool*) | median nearest seed |
| :--- | ---: | ---: | ---: | ---: |
| local 18 — *Cool* | 0.295 | 0.415 | **0.515** | 0.689 |
| inherited 11 — *Cool* | 0.219 | 0.377 | **0.502** | 0.692 |
| local 18 — *Mellow* | 0.313 | 0.451 | **0.554** | 0.730 |
| inherited 11 — *Mellow* | 0.167 | 0.282 | **0.454** | 0.766 |

**The boundaries land in the same place.** `excl_pool = 1000` cuts at a normalised distance of ~0.50–0.55 under both metrics, and the median nearest-seed distance is within a few percent. The concern in `[SPEC-DIR-200]` was that 71 weighted dimensions would shift the distance distribution enough to invalidate values tuned on 11 unweighted ones. Measured, it does not: the *shape* of the neighbourhood these parameters select is preserved.

**The one real difference is at the head, and it favours the new metric.** Passages within half the `excl_pool` boundary distance — the near-duplicate zone — drop from 16–59 under the inherited metric to **4** under the local one. With 18 characteristics it is harder for two recordings to be close on all of them, so extreme similarity is rarer and the gathered pool is less dominated by near-identical recordings. That is an improvement in pool quality that needs no parameter change to collect.

**`decay = 0.96` also stands.** Over `rand_pool = 100` it runs ×1.000 at rank 0, ×0.130 at rank 50, ×0.018 at rank 99 — a strong preference for the head without foreclosing the tail, which is what the observed winning ranks of 5, 10, 17, 24, 31, 50, 61, 99 show in practice `[SPEC-DIR-167]`.

**So the parameters are no longer marked "re-derive".** They were tuned on this library's own distance distribution `[SPEC-FD-053]`, and that distribution survived the change of metric. Revisit only if the library's composition changes substantially, not because the vector grew.

**`[SPEC-DIR-200]` The pool parameters were tuned for 8,116 passages over 11 unweighted dimensions.** Vaino uses 71 dimensions with scale normalization and reliability weighting `[SPEC-FD-040]`, which changes the distance distribution — measured between-recording spread already varies 3× across characteristics `[SPEC-FD-050]`. Carrying 1000/100/0.96 across unchanged is an assumption, not an inheritance. Re-derive against the retrieval harness before treating them as settled.

---

## 9. Open

1. **`[SPEC-DIR-210]` Eligibility is evaluated at selection time**, not projected to estimated play time — MuLibPlay's explicit `TODO`. Deferred deliberately: `[REQ-PD-110]` and the P3 acceptance test require reproducing MuLibPlay's selections, and projection would break that check before it has ever passed. Revisit as a measured divergence once reproduction is demonstrated `[GDE-PHS-030]`.
2. **`[SPEC-DIR-215]`** Whether Like-Taste should age or cap. Likes accumulate without bound; 6–8 curated seeds could be swamped.
3. **`[SPEC-DIR-220]`** Per-user Taste with a shared queue — McRhythm's multi-user model is inherited but undecided for Vaino `[REQ002 §8]`.

---

**Traceability:** `[SPEC-DIR-100..220]` · derived from `[GDE-PD-010..050]`, `[GDE-MCR-070]`, `[SPEC-FD-040]`
