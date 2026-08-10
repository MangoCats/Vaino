# SPEC008: Database Schema

**Design Specification — Tier 2**

The `vaino.db` relational model. Reconciles MuLibPlay's six-years-proven structure `[GDE-BMK-020]` with McRhythm's entity definitions `[GDE-MCR-050]` and the identity/portability rules of [SPEC006](SPEC006-data-flow-and-portability.md).

> **Related:** [GUIDE002 §2.4](../GUIDE002-rearchitecture-plan.md#2-architectural-decisions) · [SPEC005 Flavor Distance](SPEC005-flavor-distance.md) · [SPEC007 Sampo](SPEC007-sampo-architecture.md) · inherited [MCR-REQ002 Entities](../inherited/mcrhythm/MCR-REQ002-entity_definitions.md)

---

## 1. Governing Rules

**`[SPEC-SC-010]` `vaino.db` is a cache, never a source** `[SPEC-DF-010]`. Everything except listener state is re-derivable from audio by Sampo. Deleting the database must cost time, not information.

**`[SPEC-SC-015]` No field without a consumer** `[GDE-FBD-060]`. MuLibPlay carried `tempo`, `intensity`, `keyMood`, `darkLight`, `genre`, `themes` as NULL for all 8,116 rows across six years `[GDE-BMK-040]`. None are reproduced here.

**`[SPEC-SC-020]` Class D is physically segregated.** Listener state lives in tables prefixed `listener_` so the class-D export `[SPEC-DF-090]` is a table-set selection rather than a column-by-column judgement, and so "never travels with music" `[SPEC-DF-055]` is enforced by structure.

**`[SPEC-SC-025]` Provenance is non-nullable wherever a value was derived** `[GDE-FBD-020]`. The failure this prevents — 91% of v1's descriptors being silently inherited rather than computed `[GDE-V1-010]` — was invisible precisely because nothing recorded origin.

---

## 2. Identity Spine

**`[SPEC-SC-030]`** Three scopes, three keys `[SPEC-DF-030]`. Every table binds at exactly one.

```sql
-- Encoding scope: this exact audio. audio_md5 is Essentia's md5_encoded,
-- stable across tag writes [SPEC-DF-020], free at extraction [SPEC-SA-035].
CREATE TABLE files (
    file_id      INTEGER PRIMARY KEY,
    audio_md5    TEXT    NOT NULL UNIQUE,   -- identity; survives move/rename/retag
    path         TEXT    NOT NULL,          -- machine scope: never transported
    size_bytes   INTEGER NOT NULL,
    mtime        REAL    NOT NULL,          -- cheap change detection only
    format       TEXT    NOT NULL,
    duration_ms  INTEGER NOT NULL,          -- decoded, not header-claimed
    first_seen   TEXT    NOT NULL,
    last_seen    TEXT    NOT NULL
);
CREATE INDEX files_path ON files(path);

-- Recording scope: this music, any encoding. Portable across installations.
CREATE TABLE recordings (
    mbid       TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    length_ms  INTEGER,
    source     TEXT NOT NULL                -- [SPEC-SC-025]
);
```

**`[SPEC-SC-035]` `path` is deliberately not unique and never a key.** MuLibPlay's ability to relocate a moved library came from matching content, not paths `[GDE-BMK-050]`; that property is preserved by keying on `audio_md5`.

---

## 3. Passages — the Album/Radio Duality

**`[SPEC-SC-040]`** A passage is a span of audio with playback metadata `[ENT-MP-030]`. MuLibPlay's best structural idea `[GDE-BMK-030]`: **each recording in each file yields two passages**, and the Program Director selects only `radio`.

```sql
CREATE TABLE passages (
    passage_id    INTEGER PRIMARY KEY,
    file_id       INTEGER NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    kind          TEXT    NOT NULL CHECK (kind IN ('album','radio')),
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    lead_in_ms    INTEGER,                  -- computed [SPEC-SA-075]; NULL = not analysed
    lead_out_ms   INTEGER,
    gain_db       REAL,                     -- loudness match; MuLibPlay observed ~0.70-0.75 linear
    boundary_src  TEXT    NOT NULL,         -- 'computed:<algo>@<ver>' | 'manual' | 'imported:<x>'
    CHECK (end_ms > start_ms)
);
CREATE INDEX passages_file ON passages(file_id);
CREATE UNIQUE INDEX passages_span ON passages(file_id, kind, start_ms, end_ms);
```

**`[SPEC-SC-043]` Lead durations are normally milliseconds, deliberately.** Across the migrated library the lead-in median is **5 ms** and the lead-out median **946 ms**. The ramps exist primarily to mask the short, occasionally loud artifacts at a track's start and end, which needs only a few milliseconds; audible crossfade is the uncommon case, wanted where a track genuinely fades slowly and the alternative is a long near-silent gap. Near-zero overlap `[SPEC-DIR-*]` is therefore the intended behaviour, and these values should not be inflated to "enable crossfading".

Revisiting them is a future editing-UI question `[SPEC-SA-080]`, not a data defect.

**`[SPEC-SC-045]` `boundary_src` distinguishing `manual` is what makes override durable** `[SPEC-SA-080]`. Recomputation must never overwrite a `manual` row; conflict resolution ranks provenance before recency `[SPEC-DF-070]`.

**`[SPEC-SC-050]` Passage → recording is many-to-many with weights**, because a passage may contain a medley and a recording may appear in many files. Unidentified passages simply have no rows here — legal, and playable `[ENT-MP-035]`.

```sql
CREATE TABLE passage_recordings (
    passage_id  INTEGER NOT NULL REFERENCES passages(passage_id) ON DELETE CASCADE,
    mbid        TEXT    NOT NULL REFERENCES recordings(mbid),
    weight      REAL    NOT NULL DEFAULT 1.0,
    source      TEXT    NOT NULL,
    PRIMARY KEY (passage_id, mbid)
) WITHOUT ROWID;
```

Artists, releases and works follow the same shape — weighted junctions onto MusicBrainz-keyed entities — and are omitted here for length rather than being undecided.

---

## 4. Flavor

**`[SPEC-SC-060]`** Long and narrow, not 71 columns. Three reasons, each measured: partial vectors are normal (11 dims from `mulib.db`, 71 from the dump, some locally computed); provenance is **per characteristic**, not per track `[GDE-ARC-030]`; and user-defined characteristics `[GDE-MCR-060]` must be addable without DDL changes.

```sql
CREATE TABLE flavor (
    subject_kind    TEXT NOT NULL CHECK (subject_kind IN ('recording','passage')),
    subject_id      TEXT NOT NULL,          -- recordings.mbid | passages.passage_id
    characteristic  TEXT NOT NULL,          -- 'mood_happy', 'genre_dortmund', 'user.christmas'
    class           TEXT NOT NULL,          -- 'happy', 'rock', 'christmasy'
    value           REAL NOT NULL CHECK (value >= 0.0 AND value <= 1.0),
    source          TEXT NOT NULL,          -- [SPEC-SC-025]
    accuracy        REAL,                   -- measured err/beta for this characteristic
    PRIMARY KEY (subject_kind, subject_id, characteristic, class)
) WITHOUT ROWID;
CREATE INDEX flavor_subject ON flavor(subject_kind, subject_id);
```

**`[SPEC-SC-065]` Two subject kinds, deliberately.** Recording-scope flavor is portable and shareable `[SPEC-DF-040]`; passage-scope covers audio with no MusicBrainz identity, which must still be selectable. A passage prefers its own flavor and falls back to its recording's.

**`[SPEC-SC-070]` `accuracy` carries the model's measured error into the distance metric** `[SPEC-FD-120]`, so a weak characteristic degrades similarity gracefully instead of poisoning it. Populated from the model manifest.

**`[SPEC-SC-075]` Classes of a characteristic must sum to 1.0 ± 1e-4** `[MFL-DEF-040]`. Not expressible as a SQL constraint; enforced on write and audited in bulk. Verified clean on 21,636 of 21,636 instances in the sample dump.

---

## 5. Derivation Cache

**`[SPEC-SC-080]` The lowlevel cache is the most valuable table in the database** and the reason S4 and S5 are separate stages `[SPEC-SA-025]`. Extraction costs ~27 s/track and is the only step needing audio decode. Improving a classifier must re-run S5 over cached features and **never re-decode a user's library** `[GDE-CHT-045]`.

```sql
CREATE TABLE lowlevel_cache (
    audio_md5   TEXT    NOT NULL,
    start_ms    INTEGER NOT NULL,           -- 0,-1 = whole file; else the passage slice
    end_ms      INTEGER NOT NULL,
    features    BLOB    NOT NULL,           -- compressed Essentia lowlevel JSON
    extractor   TEXT    NOT NULL,           -- 'essentia-v2.1_beta2-1-ge3940c0'
    extracted_at TEXT   NOT NULL,
    PRIMARY KEY (audio_md5, start_ms, end_ms)
) WITHOUT ROWID;
```

Keyed by slice because extraction is per-passage `[SPEC-SA-090]` — **provisional**: that experiment may add a minimum-duration or eligibility rule, which would be an additional column here, not a restructuring.

**`[SPEC-SC-085]`** Identification lookups (`fpcalc` fingerprints, AcoustID and MusicBrainz responses) are cached in the same spirit: the AcoustID key is rate-limited and a single point of failure `[SPEC-SA-055]`, so a re-run must never re-query.

---

## 6. Listener State — Class D

**`[SPEC-SC-090]`** Segregated by prefix `[SPEC-SC-020]`. This is the **only irreplaceable data in the system** `[SPEC-DF-090]`; everything else re-derives from audio.

```sql
CREATE TABLE listener_play_history (          -- 37,134 rows in MuLibPlay since 2020
    play_id     INTEGER PRIMARY KEY,
    played_at   INTEGER NOT NULL,             -- unix seconds
    passage_id  INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    mbid        TEXT                          -- survives passage deletion
);
CREATE INDEX listener_play_time ON listener_play_history(played_at);

CREATE TABLE listener_preferences (           -- MuLibPlay's rotation/recovery/restraint
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('recording','artist')),
    subject_id   TEXT NOT NULL,
    rotation     REAL,                        -- log scale: 10^v hours [GDE-PD-010]
    recovery     REAL,
    restraint    REAL,
    updated_at   TEXT NOT NULL,
    PRIMARY KEY (subject_kind, subject_id)
) WITHOUT ROWID;
```

`listener_programs` / `listener_program_seeds` hold the time-of-day programs as **seed track lists** `[GDE-PD-040]`, and `listener_likes` holds weighted Like/Dislike events with their timestamps `[GDE-MCR-070]`.

**`[SPEC-SC-095]` `mbid` is denormalised into play history on purpose.** MuLibPlay did the same, and it means six years of history survives a library rescan that renumbers passages.

---

## 7. Visibility

**`[SPEC-SC-100]`** `ingest_decisions` records what each Sampo stage decided, at what confidence, and what it rejected — a durable record, not a log line `[SPEC-SA-085]`. `selection_decisions` records the Program Director's weight decomposition per choice, which is what the "Why this track?" panel reads `[GDE-CHT-030]`.

Both are append-only, bounded by retention, and are **Vaino-local**: they describe process, not music, so they never travel `[SPEC-DF-050]`.

---

## 8. Open

1. **`[SPEC-SC-110]`** Per-passage extraction may add a minimum-duration or flavor-eligibility column `[SPEC-SA-090]`. Additive.
2. **`[SPEC-SC-115]`** Cover art storage — MuLibPlay held BLOBs in `albums` (95 MB db, mostly art). Filesystem cache keyed by release MBID is likely better; not yet decided.
3. **`[SPEC-SC-120]`** Retention policy for the two decision tables.

---

**Traceability:** `[SPEC-SC-010..120]` · derived from `[GDE-ARC-040]`, `[GDE-BMK-030]`, `[SPEC-DF-030]`, `[SPEC-SA-025]`
