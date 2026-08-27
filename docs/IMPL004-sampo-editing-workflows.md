# IMPL004: Building the Editing Workflows

**Implementation Guide — the build order for [SPEC021](spec/SPEC021-waveform-boundary-editor.md) and [SPEC010 §3](spec/SPEC010-identification-review.md#3-searching-musicbrainz-directly)**

> **Related:** [IMPL003](IMPL003-sampo-console-build.md), whose Stage 5 this continues from · [SPEC013 §3.4](spec/SPEC013-sampo-console.md#34-handoff--the-players-own-pages-inside-sampos-workflow) for the handoff both features are reached through.

Two independent features, requested together `[REQ-LIB-175]`, `[REQ-LIB-180]`, sharing nothing but the `sampo-support` feature gate `[SPEC-SUI-190]` and the handoff mechanics `[IMPL003 Stage 5]` already built. Five stages; the first two are the waveform editor split at its read/write seam, the third folds an accepted edit into the library, the fourth and fifth are MusicBrainz search and the artist-credit correction it makes possible.

---

## Ordering

| stage | depends on | why here |
| :--- | :--- | :--- |
| 6 · raw audio route + waveform render | IMPL003 Stage 5 | read-only, cannot damage anything; proves decode and render before editing exists |
| 7 · dragging, preview, commit | 6 | the write half; commits to a new table, not to `passages` |
| 8 · `apply_boundary_reviews.py` | 7 | folds an accepted edit into the library, on its own schedule |
| 9 · MusicBrainz search proxy | — | independent of 6–8; can run in parallel |
| 10 · artist-only correction | 9 | needs a candidate to correct *to*, which 9 supplies |

Stages 9–10 do not block 6–8 or the reverse — they touch different tables and different pages, and the only shared resource is the rate-limited MusicBrainz proxy itself, which stage 9 builds once for both.

---

## Stage 6 — Raw audio route and waveform render

**Serve the file, render the picture, commit nothing.** `/edit/:passage_id` and `/edit/:passage_id/audio` from [SPEC021 §3](spec/SPEC021-waveform-boundary-editor.md#3-the-route-surface-all-behind-sampo-support), `Range`-aware. The page decodes client-side and draws the peak waveform with the passage's *current* (automatic or already-manual) boundaries marked, undraggable.

> **Claims:** opening `/edit/:passage_id` for a passage inside a 40-minute DAO capture shows a waveform within a few seconds, not a fetch of the whole file. The marked start/end/lead-in/lead-out match what `/edit/:passage_id/info` reports for the same passage — two views of one row, not two sources of truth. Nothing in this stage writes anywhere; it is safe to ship and use before Stage 7 exists.
>
> **Done, 2026-08-27.** `/edit/:passage_id/info` and `/edit/:passage_id/audio` (`[SPEC-SUI-201]`) added, both behind `sampo-support`; the audio route is `Range`-aware (`bytes=START-END`, `START-`, `-SUFFIX`, malformed or multi falling back to the whole file rather than erroring). `edit.js` decodes client-side and draws min/max peaks per pixel, with start/end as lines and lead-in/lead-out as shaded ramps, undraggable. 337 tests with the feature on (320 without), clippy clean both ways, docs governance clean. `decodeAudioData` speed on a real multi-minute capture is not yet measured against a browser — jsdom has no Web Audio, so the jsdom check covers the info round-trip and the not-found case, not the decode itself.

## Stage 7 — Dragging, preview, commit

**The interaction model of [SPEC021 §4](spec/SPEC021-waveform-boundary-editor.md#4-interaction-model), and the `boundary_reviews` write of §2.** Draggable markers, a Web Audio preview transport built from the same ramp formula as `[SPEC-AUD-040]`, and a commit button that posts the five draft values.

> **Claims:** dragging the end marker inward and pressing play previews the shorter span, with a fade-out that visibly matches the shaded ramp under the waveform. The shared `(position, expected gain)` fixture from `[SPEC021 §4]`'s fidelity guard passes against both the Rust formula and the JS one — the number that answers "how loud, here" is one number, computed twice, checked to agree. Committing writes exactly one `boundary_reviews` row per passage; committing twice on the same passage updates it rather than adding a second.
>
> **Done, 2026-08-27.** Four draggable markers (start, end, lead-in, lead-out) plus a gain field, a Web Audio preview built from a freshly-rendered `AudioBuffer` (sliced and faded sample-by-sample with `fade.js`, not `GainNode` automation, so the preview cannot use different interpolation than the number it is checked against), and `POST /edit/:passage_id/review` writing to `boundary_reviews` via the same `ON CONFLICT` upsert `record_review` uses. `fade.js` is its own file and route, checked against `fixtures/fade/exponential.json` from both Rust (`fade.rs`'s own test) and plain Node (`verify-skins.js`, no DOM needed). Verified end to end against a real copy of the library: passage 10068 read `edited: false`; committing `{80000, 300000, 250, 2000, -2.0}` returned `204`; `/info` then read back exactly those five values with `edited: true`; an inverted `start_ms >= end_ms` was rejected `400`. **Not verified by automated test:** the drag interaction and the preview's audible correctness, since jsdom has no Web Audio — that half needs a person with a real browser, the same limit Stage 6 already had for `decodeAudioData`.

## Stage 8 — Applying an accepted edit

**`apply_boundary_reviews.py`**, dry-run by default, `--commit` to write, per [SPEC021 §5](spec/SPEC021-waveform-boundary-editor.md#5-applying-an-accepted-edit).

> **Claims:** run without `--commit`, it reports what it would change and touches nothing. With `--commit`, an accepted `boundary_reviews` row becomes the passage's new `start_ms`/`end_ms`/`lead_in_ms`/`lead_out_ms`/`gain_db`, `boundary_src` becomes `'manual'`, and a second run makes no further change. A `lowlevel_cache` row whose span the edit invalidated is re-keyed or removed, never left pointing at a span nothing plays.
>
> **Done, 2026-08-27.** `tools/apply_boundary_reviews.py`, dry-run by default. A moved span is checked against `passages_span`'s own unique index before writing, in both modes, so a rehearsal's count is the count a real run would apply — a collision is refused and reported, not forced. The old span's `lowlevel_cache` row is **deleted, not re-keyed**, when nothing else uses it: re-keying would relabel features extracted for the old span as valid for the new one, and a wrong answer indistinguishable from a right one is worse than an honest gap a later extraction pass fills in. Verified end to end against a real copy of the library: passage 10068's edit from Stage 7's own verification, applied, left `passages` reading exactly `(80000, 300000, 250, 2000, -2.0, 'manual')` and dropped the one real `lowlevel_cache` row keyed to its old span. `tools/test_apply_boundary_reviews.py` covers the rehearsal/commit/no-op/collision/still-used-elsewhere cases against a schema copied from SPEC008, the same discipline `test_apply_reviews.py` uses.

## Stage 9 — MusicBrainz search proxy

**One route, `[SPEC-SUI-196]`'s rate-limited wrapper around `/ws/2/<kind>`**, and the review page grows a search box beside its candidate list.

> **Claims:** searching from the review page for a title AcoustID never suggested returns real MusicBrainz results, rendered as the same radio-candidate shape a fingerprint suggestion already uses. Opening two browser tabs and searching from both does not exceed roughly one request per second to musicbrainz.org — the limit lives in the proxy, not in client behaviour. Every mbid already on a review card — stored, suggested, artist, release — is a working link to its own MusicBrainz page, added without waiting for search: `[SPEC-SUI-195]` needs no new route at all.

## Stage 10 — Artist-only correction

**The one genuinely new decision shape in `[SPEC-SUI-197]`'s table**: a recording id can be right while its credited artist is wrong, and today nothing lets a person say so without also touching the recording.

> **Claims:** choosing a searched-or-suggested artist and confirming "the recording is right, the artist is not" records a decision distinct from a recording reassignment — inspectable, undoable, and appliable — that `apply_reviews.py` (extended, not duplicated per [SPEC010 §3](spec/SPEC010-identification-review.md#3-searching-musicbrainz-directly)) folds into `recording_artists` without touching `passage_recordings`. A passage whose recording id was never in question is unaffected by correcting its artist.

---

## Not on this path

**Release search beyond what a chosen recording already links to.** `[SPEC-SUI-197]`'s table names it as wanted; it is not scheduled here, because Stage 9's proxy makes it a small addition once built — a second `kind=release` search against the same route — and adding it before the proxy exists would mean building the rate-limited plumbing twice.

**Track-position correction** (`release_recordings.position`/`disc`), the fourth row of the same table. Narrower than the other three and has not yet had a real case put in front of it; speculative work here would be exactly the "unverified, not evidence" trap `[SPEC-PLAY-*]`'s own review queue was built to avoid applying to itself.

---

**Traceability:** implements `[SPEC-SUI-195..200]`, `[REQ-LIB-175]`, `[REQ-LIB-180]` · sits under [IMPL003](IMPL003-sampo-console-build.md) Stage 5
