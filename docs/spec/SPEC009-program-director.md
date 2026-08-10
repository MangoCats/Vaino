# SPEC009: Program Director

**Design Specification — Tier 2**

How Vaino chooses the next passage. Reproduces MuLibPlay's six-years-proven selection algorithm `[GDE-PD-010..050]`, extended with Like/Dislike Taste and the full 71-dimension flavor vector.

> **Related:** [REQ002 §2](REQ002-functional-requirements.md#2-program-director--pd) · [SPEC005 Flavor Distance](SPEC005-flavor-distance.md) · [SPEC008 Schema](SPEC008-database-schema.md) · inherited [MCR-SPEC005](../inherited/mcrhythm/MCR-SPEC005-program_director.md), [MCR-SPEC006 Like/Dislike](../inherited/mcrhythm/MCR-SPEC006-like_dislike.md)

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

**`[SPEC-DIR-115]` Artist pass, then track pass**, multiplicatively. For each:

1. `w = 10^(-restraint)`
2. **Hard block** if `now - last_played < seconds(rotation)` → excluded entirely.
3. Otherwise **linear recovery ramp**: `w *= clamp((age - rot) / rec, 0, 1)`.
4. Track weight multiplies its artist's weight. Related recordings block and damp too, scaled by relation strength.
5. Drop below `min_weight` → excluded.

**`[SPEC-DIR-117]` The artist weight never reaches the track weight in MuLibPlay as shipped.** Step 4 above describes the intent. The code does something else, and the difference was found only by transcribing it:

```cpp
qreal weight = pow(10.0,-restraint);          // outer
if ( eligibleArtists.contains( artistId ) )
  { weight *= eligibleArtists.value( artistId );   // writes the OUTER weight
    qreal restraint = ...;
    qreal weight = pow(10.0,-restraint);           // SHADOWS it; outer never read again
```

The inner declaration shadows the outer, so the multiplication by the artist weight is dead. `eligibleTracks.insert(trackId, weight)` sits inside that inner block. **An artist rotation block still excludes the track — that gate is a map lookup, unaffected — but a partially recovered artist does not damp its tracks at all.** The artist recovery ramp has, in six years of production, done nothing.

Two consequences, and they pull in opposite directions:

- `[REQ-PD-110]` and the P3 acceptance test require reproducing MuLibPlay's selections. A faithful port must reproduce this, or the test can never pass.
- The intended behaviour is almost certainly better. Artist recovery exists so that hearing one track by an artist gently damps the rest, and that has never happened.

Implemented as an explicit `ArtistCoupling` switch — `AsShipped` and `AsSpecified` — with both pinned by test so neither can drift into the other unnoticed. **Reproduction runs `AsShipped`; the default for listening is a decision to make once reproduction is demonstrated**, alongside `[SPEC-DIR-210]`. Note that the same block contains a second instance of the pattern: related-track recovery damping passes the *primary* track's age rather than the related track's, so it too is largely inert.

**`[SPEC-DIR-120]` Defaults matter more than they look.** Only 2,918 of 8,116 MuLibPlay tracks (36%) ever received tuned values `[GDE-BMK-020]`, so most selection runs on defaults:

| | default | = | observed tuned median |
| :--- | ---: | ---: | ---: |
| track rotation | 2.0 | 4.2 days | 2.196 (6.5 days) |
| track recovery | 2.6 | 16.6 days | 2.722 (22 days) |
| artist rotation | 1.0 | 10 hours | 1.231 (17 hours) |
| artist recovery | 1.0 | 10 hours | 1.595 (39 hours) |
| restraint | 0.0 | ×1.0 | 0.000 |

Tuned medians sit close to the defaults, which is evidence the defaults are well chosen — users nudged rather than fought them. Restraint spans −0.939 to 5.0, i.e. an 8.7× boost to a 10⁻⁵ suppression: it is the "much more / never again" knob.

**`[SPEC-DIR-125]` Passage-level filters and length bonus.** `radio` passages only `[REQ-PD-120]`. Reject shorter than 30 s, longer than 3600 s, or starting more than 10800 s into a file. Then `w *= sqrt(min(4.0, 180 s / length))` — a mild preference for ~3-minute passages, capped at 2× for short ones.

**`[SPEC-DIR-130]` Occasion multiplier — a time layer, not a flavor dimension.** User-defined characteristics `[GDE-MCR-060]` supply the *value*; a seasonal curve turns it into a multiplier:

```
w *= 1 + characteristic_value × (curve(today) − 1)
```

So `user.christmas.christmasy = 0.9` on 21 December with a curve value of 4.2 yields ×3.9; the same passage in June yields ≈×1. This keeps MuLibPlay's proven seasonal behaviour `[GDE-PD-020]` while removing the hardcoded `[C]`/`[W]`/`[S]`/`[K]` tags, and — critically — stays **legible as a single term** in the Why-this-track panel. Folding seasonality into the programme target vector was rejected for exactly that reason.

Curves are data, not code: a new occasion is a new characteristic plus a curve, with no edit to the engine.

---

## 4. Stage B — Pool Shaping

**`[SPEC-DIR-140]` Seeds define the target.** A programme is a list of 6–8 exemplar passages `[GDE-PD-040]`, down-selected to at most `max_seeds` — one per artist, least-recently-played. Naming songs beats tuning sliders, and it is the mechanism MuLibPlay users actually exercised.

**`[SPEC-DIR-145]` Prune, then gather** — both over flavor distance `[SPEC-FD-040]`:

1. **Prune** — remove passages *most unlike* every seed until the pool reaches `excl_pool`.
2. **Gather** — take the `rand_pool × 2 / seeds` passages *most like* each seed.

**`[SPEC-DIR-150]` Taste enters here, and only here** `[GDE-OPN-030]`. Like-Taste and Dislike-Taste are weighted centroids of the flavor of liked/disliked songs `[MTA-SMPL-020]`, computed per user.

- **Dislike-Taste acts as an exclusion filter**: passages within `dislike_radius` of it are removed from the pool before gathering. This is McRhythm's own suggested use `[LD-LIKE-021]`, and it is the half that needs no tuning to be useful.
- **Like-Taste acts as an additional seed**, weighted `like_seed_weight` relative to programme seeds.

Rationale: Taste is a *character* signal, so it belongs in the stage that shapes character, leaving frequency untouched `[SPEC-DIR-100]`. Treating a Like as "just another way of naming a song that defines a mood" also keeps it in the same idiom as programmes.

**`[SPEC-DIR-155]` Taste never blocks and never boosts frequency.** A disliked passage is removed from the *pool*; its rotation and restraint are unchanged. If the user later removes the Dislike, behaviour returns exactly to baseline with no residue.

**`[SPEC-DIR-158]` Cold start.** With no history, no Likes and no tuned preferences, Stage A degrades to `10^0 = 1.0` for everything and Stage B has no seeds. Selection then reduces to uniform random over eligible passages — correct, if uninspiring. A programme with seeds is the minimum for interesting behaviour, so first-run setup asks for a handful of exemplars rather than presenting an empty station.

---

## 5. Stages C & D — Flow and Roulette

**`[SPEC-DIR-160]` Flow.** Re-sort the pool by flavor distance to the **last passage already queued**, so consecutive passages blend `[GDE-PD-050]`. This is also what makes a hard programme switch acceptable `[SPEC-DIR-180]`: continuity is supplied here, not by blending programmes.

**`[SPEC-DIR-165]` Roulette.** Take the top `rand_pool`, apply rank decay `w *= decay^rank`, then pick weighted-random. Selection is by weight, not by rank — a lower-ranked passage can win, which is where the surprise lives.

---

## 6. Programme Selection

**`[SPEC-DIR-180]` Hard switch at the programme's start time**, as MuLibPlay does. Eight programmes are defined by start time `[GDE-PD-040]`; the active one is whichever most recently started. Blending was rejected: six years of production show no complaint, the flow stage already smooths transitions `[SPEC-DIR-160]`, and blending introduces a tunable nobody asked for while making "which programme am I in?" ambiguous.

**`[SPEC-DIR-185]`** Manual programme selection overrides time-of-day until the user reverts to automatic.

---

## 7. Visibility Contract

**`[SPEC-DIR-190]`** Every automatic selection writes a `selection_decisions` record `[SPEC-SC-100]` sufficient to reconstruct the choice `[REQ-VIS-100]`: artist weight and block state, track weight and ramp position, occasion multiplier with the characteristic and curve value that produced it, length bonus, final Stage-A weight, distance to each seed, Taste effect, flow distance, rank, roulette position and target — **and the runners-up that lost, with their weights**.

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
| `excl_pool` | 1000 | **Re-derive** `[SPEC-DIR-200]` |
| `rand_pool` | 100 | **Re-derive** |
| rank `decay` | 0.96 | **Re-derive** |
| `dislike_radius`, `like_seed_weight` | — | **New, unvalidated** |

**`[SPEC-DIR-200]` The pool parameters were tuned for 8,116 passages over 11 unweighted dimensions.** Vaino uses 71 dimensions with scale normalization and reliability weighting `[SPEC-FD-040]`, which changes the distance distribution — measured between-recording spread already varies 3× across characteristics `[SPEC-FD-050]`. Carrying 1000/100/0.96 across unchanged is an assumption, not an inheritance. Re-derive against the retrieval harness before treating them as settled.

---

## 9. Open

1. **`[SPEC-DIR-210]` Eligibility is evaluated at selection time**, not projected to estimated play time — MuLibPlay's explicit `TODO`. Deferred deliberately: `[REQ-PD-110]` and the P3 acceptance test require reproducing MuLibPlay's selections, and projection would break that check before it has ever passed. Revisit as a measured divergence once reproduction is demonstrated `[GDE-PHS-030]`.
2. **`[SPEC-DIR-215]`** Whether Like-Taste should age or cap. Likes accumulate without bound; 6–8 curated seeds could be swamped.
3. **`[SPEC-DIR-220]`** Per-user Taste with a shared queue — McRhythm's multi-user model is inherited but undecided for Vaino `[REQ002 §8]`.

---

**Traceability:** `[SPEC-DIR-100..220]` · derived from `[GDE-PD-010..050]`, `[GDE-MCR-070]`, `[SPEC-FD-040]`
