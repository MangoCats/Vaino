# SPEC021: Waveform Boundary Editor

**Design Specification — Tier 2 · Built 2026-08-27, see [IMPL004](../IMPL004-sampo-editing-workflows.md) Stages 6–8**

How a passage's start, end, lead-in, lead-out and gain are reviewed and corrected by hand, hearing the edit as it is made `[REQ-LIB-175]`, `[REQ-VIS-130]`, `[SPEC-SA-080]`.

> **Related:** [SPEC013 §3.4](SPEC013-sampo-console.md#34-handoff--the-players-own-pages-inside-sampos-workflow) for where this is reached from · [SPEC010](SPEC010-identification-review.md) for the sibling feature it was designed alongside · [SPEC008 §3](SPEC008-database-schema.md) for `passages` · [SPEC001 §2](SPEC001-audio-engine.md#2-mathematical-ramp-profile-models) for the fade curves this must not disagree with.

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
| `/edit/:passage_id/audio` | GET | the raw file bytes for the passage's file, `Range`-aware so the browser is not made to fetch a 40-minute capture to look at one song |
| `/edit/:passage_id/review` | POST | write a `boundary_reviews` row — body carries the five values |

**`[SPEC-SUI-201]` `/info` exists because Vaino has no server-side templating.** Every page here is a static shell compiled in with `include_str!` and fetches its own state over a small JSON route once loaded — `/why/:id` and `/review/queue` are the same shape already. There was never a way to bake a passage's boundaries into the `/edit/:passage_id` response itself, so a sibling route carries them instead.

`/edit/:passage_id/audio` is the one genuinely new capability, and it is scoped narrowly: **the file, not the passage.** Vaino already resolves a passage to a file path for playback; this reuses that resolution and streams the bytes back rather than decoding them, so it costs nothing the player does not already pay to open the file today. `Range` support lets the browser's own `<audio>`/`decodeAudioData` machinery fetch only the span it needs rather than the whole capture.

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

- Whether `decodeAudioData` on a multi-minute DAO capture (a third of this library) is fast enough to feel immediate, or needs the `Range` fetch narrowed further before decode.
- Whether dragging start/end across a passage boundary that abuts another passage in the same file needs a visible marker for the *neighbour's* span, so an edit here cannot silently create or close a gap the neighbour did not ask for.

Both are measurement questions for whoever builds this, not open design questions — the shape above does not change based on either answer.
