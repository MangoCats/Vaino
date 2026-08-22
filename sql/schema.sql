-- Vaino database schema. Executable form of SPEC008 [SPEC-SC-010..120].
--
-- Governing rules, restated because they are easy to erode:
--   * vaino.db is a CACHE, never a source [SPEC-SC-010]. Everything except
--     listener_* re-derives from audio.
--   * No field without a consumer [SPEC-SC-015]. MuLibPlay carried six columns
--     NULL for 8,116 rows across six years; none are reproduced here.
--   * Class D is segregated behind the listener_ prefix [SPEC-SC-020] so the
--     export contract is a table-set selection, not a per-column judgement.
--   * Provenance is non-nullable wherever a value was derived [SPEC-SC-025].

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ============================================================ identity spine

-- Encoding scope. audio_md5 is Essentia's md5_encoded: the MD5 of the encoded
-- audio stream with container and tags excluded, so it survives retagging,
-- renaming and moving [SPEC-DF-020]. Computable in ~70 ms via
-- `ffmpeg -i F -vn -c:a copy -f md5 -`, verified bit-identical to the
-- extractor's own value.
CREATE TABLE IF NOT EXISTS files (
    file_id      INTEGER PRIMARY KEY,
    audio_md5    TEXT    NOT NULL UNIQUE,
    path         TEXT    NOT NULL,          -- machine scope; never transported
    size_bytes   INTEGER NOT NULL,
    mtime        REAL    NOT NULL,
    format       TEXT    NOT NULL,
    duration_ms  INTEGER NOT NULL,
    first_seen   TEXT    NOT NULL,
    last_seen    TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS files_path ON files(path);

-- The file's own tags, as read from the container. Encoding scope, and part of
-- the library rather than a cache: the player resolves a display name
-- MusicBrainz -> tag -> filename, so for audio with no MusicBrainz entry this
-- is the ONLY place an artist name exists [SPEC-PL-050]. It is why tags travel
-- in a payload, and why a library built from this file could not receive one
-- until the table was named here -- added 2026-08-20, when a bundle import into
-- a fresh schema failed on "no such table: file_tags".
CREATE TABLE IF NOT EXISTS file_tags (
    file_id     INTEGER PRIMARY KEY REFERENCES files(file_id) ON DELETE CASCADE,
    title       TEXT,
    artist      TEXT,
    album       TEXT,
    track_no    INTEGER,
    disc_no     INTEGER,
    has_art     INTEGER NOT NULL DEFAULT 0,
    scanned_at  INTEGER NOT NULL
);

-- Recording scope: this music, any encoding. Portable across installations.
-- mbid is a MusicBrainz recording MBID, or 'local:<n>' where identification
-- has not happened -- unidentified audio must still be playable [ENT-MP-035].
CREATE TABLE IF NOT EXISTS recordings (
    mbid       TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    length_ms  INTEGER,
    source     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artists (
    mbid       TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    sort_name  TEXT,
    source     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS releases (
    mbid          TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    release_date  TEXT,
    source        TEXT NOT NULL
);

-- ================================================================= passages

-- Each recording in each file yields TWO passages [SPEC-SC-040]: 'album' with
-- full boundaries, and 'radio' trimmed with segue points and gain. The Program
-- Director selects only 'radio' [REQ-PD-120].
CREATE TABLE IF NOT EXISTS passages (
    passage_id    INTEGER PRIMARY KEY,
    file_id       INTEGER NOT NULL REFERENCES files(file_id) ON DELETE CASCADE,
    kind          TEXT    NOT NULL CHECK (kind IN ('album','radio')),
    start_ms      INTEGER NOT NULL,
    end_ms        INTEGER NOT NULL,
    lead_in_ms    INTEGER,
    lead_out_ms   INTEGER,
    gain_db       REAL,
    -- 'manual' outranks everything and is never silently recomputed
    boundary_src  TEXT    NOT NULL,
    CHECK (end_ms > start_ms)
);
CREATE INDEX IF NOT EXISTS passages_file ON passages(file_id);
CREATE UNIQUE INDEX IF NOT EXISTS passages_span ON passages(file_id, kind, start_ms, end_ms);

-- Many-to-many with weights: a passage may hold a medley, a recording may
-- appear in many files. No rows here == unidentified, which is legal.
CREATE TABLE IF NOT EXISTS passage_recordings (
    passage_id  INTEGER NOT NULL REFERENCES passages(passage_id) ON DELETE CASCADE,
    mbid        TEXT    NOT NULL REFERENCES recordings(mbid),
    weight      REAL    NOT NULL DEFAULT 1.0,
    source      TEXT    NOT NULL,
    PRIMARY KEY (passage_id, mbid)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS recording_artists (
    mbid         TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    artist_mbid  TEXT NOT NULL REFERENCES artists(mbid),
    weight       REAL NOT NULL DEFAULT 1.0,
    source       TEXT NOT NULL,
    PRIMARY KEY (mbid, artist_mbid)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS release_recordings (
    release_mbid  TEXT NOT NULL REFERENCES releases(mbid) ON DELETE CASCADE,
    mbid          TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    position      INTEGER,
    source        TEXT NOT NULL,
    PRIMARY KEY (release_mbid, mbid)
) WITHOUT ROWID;

-- Related recordings: block and damp each other in selection [SPEC-DIR-115].
CREATE TABLE IF NOT EXISTS recording_relations (
    mbid          TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    related_mbid  TEXT NOT NULL REFERENCES recordings(mbid) ON DELETE CASCADE,
    strength      REAL NOT NULL DEFAULT 1.0,
    source        TEXT NOT NULL,
    PRIMARY KEY (mbid, related_mbid)
) WITHOUT ROWID;

-- =================================================================== flavor

-- Long and narrow, not 71 columns [SPEC-SC-060]: partial vectors are normal,
-- provenance is per characteristic, and user-defined characteristics must be
-- addable without DDL changes.
CREATE TABLE IF NOT EXISTS flavor (
    subject_kind    TEXT NOT NULL CHECK (subject_kind IN ('recording','passage')),
    subject_id      TEXT NOT NULL,
    characteristic  TEXT NOT NULL,
    class           TEXT NOT NULL,
    value           REAL NOT NULL CHECK (value >= 0.0 AND value <= 1.0),
    source          TEXT NOT NULL,
    accuracy        REAL,                   -- measured err/beta [SPEC-FD-120]
    PRIMARY KEY (subject_kind, subject_id, characteristic, class)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS flavor_subject ON flavor(subject_kind, subject_id);

-- Corpus constants for the distance metric [SPEC-FD-052]. Stored, not hardcoded:
-- beta is a property of the corpus being searched, and measuring it on the wrong
-- corpus has already produced two wrong answers [LOG-I6-030].
CREATE TABLE IF NOT EXISTS flavor_constants (
    characteristic  TEXT PRIMARY KEY,
    beta            REAL NOT NULL,
    reliability     REAL NOT NULL,
    measured_on     TEXT NOT NULL,
    measured_at     TEXT NOT NULL
) WITHOUT ROWID;

-- ============================================================ derivation cache

-- The most valuable table here [SPEC-SC-080]. Extraction costs ~27 s/track and
-- is the only stage needing audio decode; improving a classifier re-runs
-- classification over these and never re-decodes a user's library.
CREATE TABLE IF NOT EXISTS lowlevel_cache (
    audio_md5     TEXT    NOT NULL,
    start_ms      INTEGER NOT NULL,         -- 0 / -1 == whole file
    end_ms        INTEGER NOT NULL,
    features      BLOB    NOT NULL,         -- zlib-compressed Essentia JSON
    extractor     TEXT    NOT NULL,
    extracted_at  TEXT    NOT NULL,
    PRIMARY KEY (audio_md5, start_ms, end_ms)
) WITHOUT ROWID;

-- The AcoustID key is rate-limited and a single point of failure
-- [SPEC-SA-055]; a re-run must never re-query.
CREATE TABLE IF NOT EXISTS identification_cache (
    audio_md5    TEXT NOT NULL,
    service      TEXT NOT NULL,             -- 'fpcalc' | 'acoustid' | 'musicbrainz'
    request_key  TEXT NOT NULL,
    response     BLOB NOT NULL,
    fetched_at   TEXT NOT NULL,
    PRIMARY KEY (audio_md5, service, request_key)
) WITHOUT ROWID;

-- =========================================================== listener state
-- CLASS D [SPEC-SC-020]. The only irreplaceable data in the system. Never
-- travels with music [SPEC-DF-055]; exported on a schedule [SPEC-DF-094].

CREATE TABLE IF NOT EXISTS listener_play_history (
    play_id     INTEGER PRIMARY KEY,
    played_at   INTEGER NOT NULL,           -- unix seconds
    passage_id  INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    -- denormalised on purpose: six years of history must survive a rescan
    -- that renumbers passages [SPEC-SC-095]
    mbid        TEXT
);
CREATE INDEX IF NOT EXISTS listener_play_time ON listener_play_history(played_at);
CREATE INDEX IF NOT EXISTS listener_play_mbid ON listener_play_history(mbid);

-- Words, keyed by the recording rather than the passage [SPEC-LYR-020]. A
-- recording's words do not change because it was ripped twice, and two passages
-- of one recording share them -- the same scope `flavor` and `recordings` use.
--
-- Class C: they travel [SPEC-LYR-025]. Reference data about a recording, not an
-- account of one person's listening.
CREATE TABLE IF NOT EXISTS lyrics (
    mbid       TEXT PRIMARY KEY REFERENCES recordings(mbid),
    text       TEXT NOT NULL,
    source     TEXT NOT NULL,           -- where they came from, e.g. 'mulibplay'
    fetched_at TEXT NOT NULL
);

-- Declining a song is not a play [SPEC-PLAY-010], and this table is why it can
-- still matter. Recorded ONLY so a passage the listener rejected is not offered
-- back immediately [SPEC-PLAY-050]; it feeds no ramp, no artist damping and no
-- count. Class D, never travels [SPEC-DF-055].
--
-- `kind` separates the two ways of declining, because they earn different
-- windows [SPEC-PLAY-055]: 'skip' is a passage stopped after it began sounding,
-- 'dequeue' is one removed from the queue before it ever played.
CREATE TABLE IF NOT EXISTS listener_rejections (
    rejection_id INTEGER PRIMARY KEY,
    rejected_at  INTEGER NOT NULL,          -- unix seconds
    kind         TEXT NOT NULL,             -- 'skip' | 'dequeue'
    passage_id   INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    -- denormalised for the same reason as listener_play_history [SPEC-SC-095]
    mbid         TEXT
);
CREATE INDEX IF NOT EXISTS listener_reject_mbid ON listener_rejections(mbid, kind);

-- rotation/recovery are log-scale: seconds = 10^v * 3600 [SPEC-DIR-110].
-- Defaults when absent: track 2.0/2.6, artist 1.0/1.0, restraint 0.0
-- [SPEC-DIR-120] -- they matter, since only 36% of MuLibPlay tracks were tuned.
CREATE TABLE IF NOT EXISTS listener_preferences (
    subject_kind  TEXT NOT NULL CHECK (subject_kind IN ('recording','artist')),
    subject_id    TEXT NOT NULL,
    rotation      REAL,
    recovery      REAL,
    restraint     REAL,
    updated_at    TEXT NOT NULL,
    PRIMARY KEY (subject_kind, subject_id)
) WITHOUT ROWID;

-- Master multipliers over every block and ramp DURATION [SPEC-DIR-118]. One
-- dial each for artists and tracks: 1.0 is inert, 0.5 halves every window,
-- 2.0 doubles it. Per-subject values are log-scale, so "everything a bit
-- sooner" is not otherwise expressible without editing thousands of rows.
-- The range is enforced here as well as in code -- a stored value out of
-- range would silently change selection everywhere.
CREATE TABLE IF NOT EXISTS listener_settings (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    artist_time_scale REAL NOT NULL DEFAULT 1.0
                      CHECK (artist_time_scale BETWEEN 0.0001 AND 100.0),
    track_time_scale  REAL NOT NULL DEFAULT 1.0
                      CHECK (track_time_scale  BETWEEN 0.0001 AND 100.0),
    -- Programme start times are wall-clock [SPEC-DIR-180]: a 22:00 programme
    -- means ten at night where the listener is. std has no timezone, so the
    -- appliance stores its offset rather than the player guessing.
    utc_offset_minutes INTEGER NOT NULL DEFAULT 0
                      CHECK (utc_offset_minutes BETWEEN -1440 AND 1440),
    updated_at        TEXT NOT NULL
);

-- Seasonal curves [SPEC-DIR-130]. Data, not code: a new occasion is rows here
-- plus flavor values, with no edit to the engine. MuLibPlay hardcoded four
-- ([C] [W] [S] [K]) into a switch, which is why it had exactly four.
-- The value side lives in `flavor` -- an occasion IS a user characteristic.
CREATE TABLE IF NOT EXISTS listener_occasions (
    characteristic  TEXT NOT NULL,          -- e.g. 'user.christmas'
    class           TEXT NOT NULL,          -- e.g. 'christmasy'
    interp          TEXT NOT NULL DEFAULT 'step'
                    CHECK (interp IN ('step','linear')),
    PRIMARY KEY (characteristic, class)
) WITHOUT ROWID;

-- Control points around a wrapped year. Multiplier must be positive: zero or
-- below would be a suppression the weight product cannot recover from, and
-- negative would invert it.
CREATE TABLE IF NOT EXISTS listener_occasion_points (
    characteristic  TEXT    NOT NULL,
    class           TEXT    NOT NULL,
    month           INTEGER NOT NULL CHECK (month BETWEEN 1 AND 12),
    day             INTEGER NOT NULL CHECK (day BETWEEN 1 AND 31),
    multiplier      REAL    NOT NULL CHECK (multiplier > 0.0),
    PRIMARY KEY (characteristic, class, month, day),
    FOREIGN KEY (characteristic, class)
        REFERENCES listener_occasions(characteristic, class) ON DELETE CASCADE
) WITHOUT ROWID;

-- A programme is a list of exemplar passages, not tuned parameters
-- [SPEC-DIR-140].
CREATE TABLE IF NOT EXISTS listener_programs (
    program_id  INTEGER PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    start_time  TEXT                        -- 'HH:MM', NULL = manual only
);
CREATE TABLE IF NOT EXISTS listener_program_seeds (
    program_id  INTEGER NOT NULL REFERENCES listener_programs(program_id) ON DELETE CASCADE,
    mbid        TEXT    NOT NULL,
    position    INTEGER NOT NULL,
    PRIMARY KEY (program_id, mbid)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS listener_likes (
    like_id     INTEGER PRIMARY KEY,
    mbid        TEXT    NOT NULL,
    weight      REAL    NOT NULL,           -- negative == dislike
    recorded_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS listener_likes_mbid ON listener_likes(mbid);

-- ================================================================ visibility
-- Describes process, not music, so never travels [SPEC-SC-100].

CREATE TABLE IF NOT EXISTS ingest_decisions (
    decision_id  INTEGER PRIMARY KEY,
    audio_md5    TEXT NOT NULL,
    stage        TEXT NOT NULL,
    outcome      TEXT NOT NULL,
    confidence   REAL,
    detail       BLOB,                      -- candidates considered and rejected
    decided_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ingest_decisions_md5 ON ingest_decisions(audio_md5);

CREATE TABLE IF NOT EXISTS selection_decisions (
    decision_id  INTEGER PRIMARY KEY,
    selected_at  INTEGER NOT NULL,
    passage_id   INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    program_id   INTEGER REFERENCES listener_programs(program_id) ON DELETE SET NULL,
    -- full weight decomposition plus the runners-up that lost [REQ-VIS-100]
    detail       BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS selection_decisions_time ON selection_decisions(selected_at);

-- ============================================================ player state

-- Resume point across restart [REQ-AUD-140]. Single row.
--
-- Deliberately NOT listener_*: this is operational state, not listener history.
-- Losing it costs one track position, so it is excluded from the class-D
-- export [SPEC-DF-090], which exists for data that cannot be reconstructed.
--
-- Playback has two states only, playing and paused [REQ-AUD-142]; there is no
-- "stopped", so `playing` is a boolean rather than an enum.
CREATE TABLE IF NOT EXISTS player_state (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    passage_id   INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    position_ms  INTEGER NOT NULL DEFAULT 0,
    playing      INTEGER NOT NULL DEFAULT 0,
    volume       REAL    NOT NULL DEFAULT 1.0,
    updated_at   TEXT    NOT NULL
);

-- ================================================================== metadata

CREATE TABLE IF NOT EXISTS schema_meta (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
) WITHOUT ROWID;
INSERT OR REPLACE INTO schema_meta VALUES ('schema_version','1');
INSERT OR REPLACE INTO schema_meta VALUES ('spec','SPEC008');
