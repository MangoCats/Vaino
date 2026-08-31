# SPEC021: Waveform Boundary Editor

**Design Specification — Tier 2 · Built 2026-08-27, see [IMPL006](../IMPL006-sampo-editing-workflows.md) Stages 6–8**

How a passage's start, end, lead-in, lead-out and gain are reviewed and corrected by hand, hearing the edit as it is made `[REQ-LIB-175]`, `[REQ-VIS-130]`, `[SPEC-SA-080]`.

> **Related:** [SPEC013 §3.4](SPEC013-sampo-console.md#34-handoff--the-players-own-pages-inside-sampos-workflow) for where this is reached from · [SPEC010](SPEC010-identification-review.md) for the sibling feature it was designed alongside · [SPEC008 §3](SPEC008-database-schema.md) for `passages` · [`player/src/fade.rs`](../../player/src/fade.rs) for the fade curves this must not disagree with (design context in the inherited [`MCR-SPEC002-crossfade.md`](../inherited/mcrhythm/MCR-SPEC002-crossfade.md), `[INH-*]` — its curve *formulas* are not what `fade.rs` implements, only its lead/fade point model).

---

## 1. Where it lives, and why that is settled already

Where this lives was decided already, in `[SPEC-SUI-135]`: built in Vaino, reached from Sampo's profile page, behind `sampo-support` `[SPEC-SUI-190]`. This document is the "how", not the "where."

**It does not need Vaino's real-time mixer.** The first instinct — reuse the engine's own decoder and crossfade path so the preview is provably what production plays — turns out not to be the cheapest way to get that guarantee, and it puts editing traffic through the same loop `[PI3-API-030]` exists to keep uninterruptible. What the editor actually needs is: the raw audio bytes, and the same fade-curve arithmetic the engine uses, applied client-side. Vaino already has to serve the first; the second is a few lines of `fade.rs` ported to JS once and covered by a shared test vector, not a new mode threaded through the tick loop.

**It does not write `passages`.** `[SPEC-SC-045]`'s `boundary_src = 'manual'` outranking automation is a promise about the *library*, and the library is Sampo's to write `[SPEC-SA-015]`. The identification-review feature already solved this exact problem for recording ids — a decision table Vaino owns, applied to the library by a separate, deliberate Sampo-side step — and boundary edits use the same shape rather than inventing a second one.

---

## 2. Data model

**`[SPEC-SUI-200]` A new table, `boundary_reviews`, shaped like `id_reviews`:**

```sql
CREATE TABLE boundary_reviews (
    passage_id    INTEGER PRIMARY KEY,
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    lead_in_ms    INTEGER,
    lead_out_ms   INTEGER,
    gain_db       REAL,
    decided_at    TEXT NOT NULL,
    applied_at    TEXT
);
```

Created by Vaino on first write, the same way `id_reviews` is `[SPEC-SC-020]`-adjacent player-owned state, not Sampo schema. `applied_at` carries the same refusal-to-quietly-withdraw rule as `id_reviews.applied_at`: once `apply_boundary_reviews.py` has rewritten `passages`, the row can be reverted but not silently deleted, for the same reason — the evidence a manual edit overrode should survive the edit.

**No `previous_*` columns.** Unlike a recording reassignment, the *original* automatic values are always recoverable by re-running the amplitude/segmentation pass that produced them — they are derived data, not the only copy of a fact. A revert re-derives rather than restores.

---

## 3. The route surface, all behind `sampo-support`

| route | method | does |
| :--- | :--- | :--- |
| `/edit/:passage_id` | GET | the editor page |
| `/edit/:passage_id/info` | GET | the passage's current boundaries as JSON |
| `/edit/:passage_id/audio` | GET | a decoded WAV window, `?from_ms=&to_ms=`, around the passage — never the raw file, see `[SPEC-SUI-224]` |
| `/edit/:passage_id/review` | POST | write a `boundary_reviews` row — body carries the five values |

**`[SPEC-SUI-201]` `/info` exists because Vaino has no server-side templating.** Every page here is a static shell compiled in with `include_str!` and fetches its own state over a small JSON route once loaded — `/why/:id` and `/review/queue` are the same shape already. There was never a way to bake a passage's boundaries into the `/edit/:passage_id` response itself, so a sibling route carries them instead.

**`/edit/:passage_id/audio` decodes through `PassageDecoder` and returns a WAV, scoped to a bounded window, not the raw file `[SPEC-SUI-224]`.** The original design served the raw file's bytes, `Range`-aware in principle — a first draft, corrected once a real 4-hour DAO capture proved decoding a whole file client-side unusable rather than merely slow. See `[SPEC-SUI-224]`'s own addendum below for the full account; §4 and §6 below describe the interaction model this route serves, not this history.

**Reached the same way review is.** `?passage=` deep-links `/edit` exactly as it deep-links `/review` `[SPEC-SUI-150]`; the profile page's handoff box gets a second button once this exists, not a second mechanism.

---

## 4. Interaction model

**One canvas, four draggable markers, one gain control.**

- **Waveform**: decoded once client-side via `decodeAudioData` on the fetched bytes, reduced to a min/max pair per horizontal pixel — the standard peak-rendering approach, cheap even for a multi-minute capture because it runs once per load, not per frame.
- **Start / End**: two vertical handles bounding the passage. Dragging either previews from the new boundary on release, not on every pixel of motion — scrubbing a waveform at 60 fps while also decoding audio on every frame is where a "real-time" editor stops feeling real-time.
- **Lead-in / Lead-out**: two secondary handles, constrained inside [start, end], shown as a shaded ramp rather than a bare line — the shading *is* the fade curve, drawn from the same formula the preview player applies, so "what will this sound like" and "what does this look like" are one picture.
- **Gain**: a numeric field beside the waveform, not a marker on it — gain is not a position, and drawing it as one (a common mistake) invites confusing loudness with placement.
- **Preview**: a transport bar under the waveform — play/pause, and a playhead that moves during playback and can be dragged to seek — built from a Web Audio `AudioBufferSourceNode` slicing the decoded buffer at the current draft boundaries and applying the current draft fades and gain. This is what makes it "real-time": every drag changes what the next press of play actually sounds like, with no round trip to the server.
- **Commit**: one button, disabled until something has changed, posting the five draft values to `/edit/:passage_id/review`. There is no autosave — an edit is a deliberate decision, matching `id_reviews`' own "recorded, not applied" posture `[SPEC-SUI-140]`.

**The fade curve must be the same formula in both places, or the preview lies.** `[SPEC-AUD-040]` already states the ramp profiles the mixer uses; this editor's JS implements the identical formula, and a shared table of `(position, expected gain)` pairs — computed once in Rust, checked once in JS against the same numbers — is the guard against the two drifting apart silently, the same failure class `[SPEC-PLAY-030]` was written to close for two backends agreeing about what a play is.

---

## 5. Applying an accepted edit

**`apply_boundary_reviews.py`, alongside `apply_reviews.py`, not folded into it.** The two tools share a shape — dry-run by default, `--commit` to write, refuse an already-applied row rather than silently re-apply — but touch different tables under different constraints (`passages.boundary_src` here, `passage_recordings`/`recording_artists` there), and `apply_reviews.py` is already sized to its own job. Rewrites `passages` for a `boundary_reviews` row not yet applied: `start_ms`, `end_ms`, `lead_in_ms`, `lead_out_ms`, `gain_db`, and sets `boundary_src = 'manual'` — which, per `[SPEC-SC-045]`, is what makes the override outrank any future recomputation permanently.

`lowlevel_cache` is keyed `(audio_md5, start_ms, end_ms)` `[SPEC-SC-080]`. A boundary edit that moves `start_ms`/`end_ms` orphans the cached features for the old span exactly as `[REQ-LIB-145]`'s duration repair did — `apply_boundary_reviews.py` must decide, per edit, whether to re-key or invalidate, not silently leave a stale cache row pointing at a span nothing plays anymore.

---

## 6. Not yet measured

- ~~Whether `decodeAudioData` on a multi-minute DAO capture (a third of this library) is fast enough to feel immediate, or needs the `Range` fetch narrowed further before decode.~~ Measured, 2026-08-30, against a real 4h05m capture: not fast enough, not even close — see `[SPEC-SUI-224]`. `/edit/:passage_id/audio` now decodes a bounded window server-side rather than the whole file client-side, closing this rather than narrowing it.
- Whether dragging start/end across a passage boundary that abuts another passage in the same file needs a visible marker for the *neighbour's* span, so an edit here cannot silently create or close a gap the neighbour did not ask for.
- Whether the fixed ±60s window `[SPEC-SUI-224]` needs an on-demand "load more" when an edit genuinely needs to reach further — not yet requested, and the honest facts-line note stands in for it until it is.

Measurement questions for whoever builds this, not open design questions — the shape above does not change based on any answer.

---

## 7. Usability, corrected against real use — built 2026-08-30

Working actual boundaries on a real multi-minute file surfaced that §4's design, while correctly *specified*, was hard to actually *use*: a fixed-width canvas mapped the whole file to one screen, so a lead-in a few hundred milliseconds wide was a few pixels wide; the only way to grab a marker was a blind ~10px guess on a bare line; there was no way back from a mistake; and the server's own transport kept playing underneath the browser's own preview with no way to silence it from this page.

**`[SPEC-SUI-217]` Two audio engines, one silenced on entry.** Vaino's engine plays through the server's own audio device continuously, independent of any browser tab — a fact this document did not name, because it did not need to for the interaction model in §4 to be correct on paper. The editor's preview (§4's `AudioBufferSourceNode`) is a *second*, independent stream to the same output. Rather than threading the editor through the real-time mixer to unify them — the exact cost §1 already ruled out — `edit.js` posts `POST /command/pause` once on load (the same route the main skin's own pause button uses) and says so in the page. No auto-resume on unload: unreliable to detect, and the main page's own Play button is one click away.

**`[SPEC-SUI-218]` A viewport, not just a buffer.** `msOf`/`draw` now map the canvas against `view = {fromMs, toMs}` rather than always `[0, totalMs()]` — zoom (buttons, and mouse wheel centred on the cursor) narrows or widens the span; **Whole passage** and **Whole file** jump to the two spans actually worth naming; a drag that starts on empty canvas (not a marker) pans the viewport, told apart from a click-to-jump by whether the pointer actually moved past a small threshold before release. Below the top ~28px "knob rail" a canvas click always means pan-or-jump, never a marker grab, closing the ambiguity between "drag a boundary" and "seek here" that a single bare-line hit-test left the person to guess at.

**`[SPEC-SUI-219]` A knob looks grabbable; a numeric field is exact.** Markers gained an actual circular knob shape near the top of the canvas rather than being a bare vertical line indistinguishable from the waveform beside it. A second row of plain ms fields (start/end/lead-in/lead-out) sits beside the existing gain field, wired through the identical clamps `applyDrag` already enforces — the same five values, typed instead of dragged, which is most of what "how do I place these" was actually asking for.

**`[SPEC-SUI-220]` One undo stack, pushed on completed edits only.** A pre-change snapshot of the draft is pushed before a drag begins, and before a field's `change` fires — never on a drag's own motion, matching the "preview on release, not per-pixel" rule §4 already stated for playback. Undo pops the stack, stops any playing preview so heard and shown state never disagree, and restores the prior values.

None of the above changes what §2's `boundary_reviews` row looks like, what `/edit/:passage_id/review` accepts, or what `apply_boundary_reviews.py` does with it — this section is entirely the interaction layer in §4, corrected against actually using it.

> **Bug fix, 2026-08-30 — `[SPEC-SUI-217]` was necessary but not sufficient.** Pausing the main transport stopped the *second engine*; it did not stop this one from silently losing count of its own. `playFrom`'s `onended` handler closed over the shared `source` variable rather than the specific node it belonged to: stopping a node schedules its `ended` event, which does not fire until sometime *after* `.stop()` returns, and by then a later click may already have started a newer node and pointed `source` at it. The stale callback still fired, still nulled `source` — pointing it at nothing, not at the node it actually belonged to — and the node it was supposed to describe kept sounding, un-stoppable, because `stopPreview()`'s next call found `source` already `null` and had nothing left to call `.stop()` on. Each click that raced this window left one more node playing forever, which is exactly "starting another stream" from where a person sits. Fixed by identity: `onended` now clears `source` only `if (source === node)` for the specific node it was attached to, and `stopPreview()` detaches `onended` from the node it is about to stop before stopping it, so a stale callback can no longer fire against a reference that has already moved on.

> **Two more, found live against Slow Ride, 2026-08-30.**
>
> **`[SPEC-SUI-221]` A marker off the current edge is indistinguishable from a marker that does not exist.** `msOf`/`draw` map the canvas against `view`, and nothing marks the difference between "off to the side" and "absent" — a lead-in near the start and a lead-out four minutes later are rarely both in frame at any zoom worth looking closely at either with. Each precise field now reveals its own marker on `focus`, before a single character is typed, and again after a `change`: `ensureVisible` re-centres the view on it, and for the two lead fields also re-*zooms* when the marker, though technically on screen, is too small a fraction of a wide view to actually read — a 4.5s lead-out on a five-minute whole-passage view is a handful of pixels, present and unreadable, which looks exactly like absent from where a person sits.
>
> **`[SPEC-SUI-222]` A marker exactly on the view's own boundary was being clipped in half.** `start_ms` is `0` and `end_ms` is the file's own duration for most passages in this library — one track, one file — so "Whole passage" and "Whole file" routinely place both blue markers *exactly* on the canvas edge. A knob drawn centred there is half off-canvas; a line there reads as the canvas's own border, not as a marker. `EDGE_PAD_CSS` reserves a small margin on each side (2px — enough that a knob is never clipped, kept tight per feedback once the fix was confirmed working), shared by drawing, hit-testing, panning and wheel-zoom math alike — one padded coordinate mapping (`msToX`/`xToMs`), not a fix applied to rendering alone while hit-testing quietly disagreed with it.

> **`[SPEC-SUI-223]` Built 2026-08-30 — a per-passage flavor refresh, and a suggestion before saving a large move.** Correctly deleting a stale `lowlevel_cache` row (the previous addendum's bug fixes led directly here) left flavor genuinely empty until something re-extracts it, and there was no way to ask for that on one passage — only library- or folder-wide, always as part of a heavier pipeline. `extract_library.py` gained `--passage <id>`; `tools/jobs.py`'s `analyze-flavor` kind and `POST /api/analyze-flavor` reach it from a new button on the passage's own profile page. Small edits do not need this — a trim of a few hundred ms does not make flavor meaningfully wrong — so `edit.js` only suggests it: `loadedBoundary`, captured once at page load (distinct from `base`, which resets on every save), compared against the live draft; a move past 5000ms on either boundary shows a plain-language note beside Save, naming where the actual action lives, since Vaino cannot invoke Sampo's tools itself. Verified live: Slow Ride's own flavor, emptied by the duration repair's own correct cache invalidation, restored by the new button in one run.

> **`[SPEC-SUI-224]` Built 2026-08-30 — §3's own audio route was serving the whole file, for real, to a browser that could not cope.** Reported live: `/profile/16379` showed no waveform and an unresponsive Play. Root cause: passage 16379 is a ~2.75-minute segmented slice out of `GoodbyeYellowBrickRoad.mp3` — a **324.7 MB, 4h05m single-file DAO capture**. `edit.js` never actually sent a `Range` header (§3's own design allowed for one, but nothing ever used it), so every load fetched the *entire* file and asked `decodeAudioData` for it — roughly 10 GB of interleaved f32 PCM for four hours of stereo audio, exactly the risk §6 named as unmeasured, now confirmed on a file measured in hours rather than "multi-minute." It also contradicted `[GDE-FBD-010]`, this project's own first principle: audio is never decoded whole.
>
> **Fixed by no longer serving raw file bytes at all.** `/edit/:id/audio` now takes `from_ms`/`to_ms` and decodes exactly that span through `PassageDecoder` — the same seek-accurate, bounded-memory decoder the real player already uses — returning a WAV rather than a slice of the original compressed file. This also sidesteps something this session's own duration audit already proved unsafe: estimating a *byte* range from a *time* range via bitrate math is the same wrong-for-VBR mistake `[REQ-LIB-145]` exists to warn against; seeking by time through `PassageDecoder` means that arithmetic is never done at all. `edit.js` requests `[start_ms − 60s, end_ms + 60s]`, clamped to the file's own bounds — for the near-totality of the library (`start_ms=0`, `end_ms=file_ms` already) this equals the whole file regardless, so an ordinary passage sees no practical change beyond "now a WAV, decoded server-side." The **"Whole file" button is relabelled "Zoom out"** — for a bounded window it no longer means the literal whole file — and the facts line names the window explicitly (`showing ±60s around this passage, not the whole file`) whenever it is narrower than the file actually is. Expanding the window on request is real future work, not solved here, the same honest-deferral shape §6 already uses for the neighbour-passage question. `read_audio_range`/`parse_byte_range`/`mime_for_ext` and their tests are gone with the route they existed for; verified live against the actual reported passage, not only against a synthetic fixture.

> **`[SPEC-SUI-225]` Built 2026-08-30 — a change that is "in view" is not necessarily legible.** Reported live: editing `end_ms` didn't visibly move anything on the waveform until `lead_out_ms` was also touched. `draw()` genuinely ran on every edit — the actual fault was `ensureVisible(draft.end_ms)` re-centring the view only when the point fell *off* screen, never re-zooming when it was on screen but sub-pixel: a 300ms nudge on a wide "Zoom out" view moves the blue end-line by less than a pixel, indistinguishable from nothing having happened, and with `lead_out_ms=0` (a common case — most tracks end abruptly) there was no amber band to notice moving either. `revealSpan` — already used for the lead fields, scaling breathing room to the lead's own length — now also drives `startms`/`endms`: scaled instead to *how far this particular edit just moved the value*, since a start/end position's own absolute magnitude (a file-relative ms count, often in the millions) is meaningless as a zoom hint the way a lead's own length is.
