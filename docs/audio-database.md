# Audio Database & Selection Intelligence

This document defines the data schema concept, track metadata parameters, audio characteristic descriptors, and context-aware auto-playlist selection engine for **Vaino**.

---

## 🗄️ Database Role & Overview

Alongside the audio media files, Vaino maintains a local database (SQLite/embedded) that acts as the "brain" for playback execution and track selection. The database stores:
1. Physical audio file properties & trim/crossfade boundaries.
2. Global metadata & MusicBrainz identifiers.
3. AudioBrainz-inspired high-level audio descriptors.
4. Play history logs & listener rating signals.

---

## 📋 Track Metadata & Playback Boundary Schema Concept

To enable continuous radio playback without abrupt silent gaps or jarring transitions, every track entry defines custom playback boundaries and crossfade profiles.

```sql
-- Concept Track Schema
CREATE TABLE tracks (
    id                   TEXT PRIMARY KEY,
    file_path            TEXT NOT NULL,
    file_format          TEXT NOT NULL,          -- 'FLAC', 'MP3', 'WAV', 'DAO'
    title                TEXT NOT NULL,
    artist               TEXT NOT NULL,
    album                TEXT,
    
    -- Trimming & Boundary Control
    start_offset_ms      INTEGER DEFAULT 0,      -- Custom start time (crucial for DAO files)
    end_offset_ms        INTEGER,                -- Custom end time (NULL = end of file)
    
    -- Crossfade & Ramp Profiles
    fade_in_ms           INTEGER DEFAULT 2000,   -- Fade-in duration
    fade_out_ms          INTEGER DEFAULT 3000,   -- Fade-out duration
    fade_ramp_profile    TEXT DEFAULT 'S_CURVE', -- 'LINEAR', 'EXPONENTIAL', 'S_CURVE'
    
    -- External Identifiers
    musicbrainz_track_id TEXT,
    musicbrainz_album_id TEXT
);
```

### DAO (Disc-At-Once) Support
For long capture files containing an entire CD or broadcast, multiple `tracks` records point to the same physical `file_path`, using `start_offset_ms` and `end_offset_ms` to isolate individual songs seamlessly.

---

## 🔍 Automated MusicBrainz Identifier Database Construction

Vaino automatically populates and builds a local offline MusicBrainz identifier database for all songs and passages contained in local music folders:

```
 [ Local Audio File / DAO Passage ]
                 │
                 ▼
 [ 1. Chromaprint Fingerprint (`fpcalc`) ]
                 │
                 ▼
 [ 2. AcoustID API Query ] ──► Returns Candidate MusicBrainz Recording ID (`recording_mbid`)
                 │
                 ▼
 [ 3. MusicBrainz API Lookup ] ──► Fetches Canonical Track, Album Release, Artist & Genre Tags
                 │
                 ▼
 [ 4. SQLite Offline Storage ] ──► Persists `recording_mbid`, `release_mbid`, `artist_mbid`
                                    into local `vaino.db` for offline 24/7 playback
```

1. **Automated Audio Fingerprinting**: Generates Chromaprint fingerprints for single-track files and individual passages of DAO continuous album files.
2. **AcoustID & MusicBrainz Querying**: Automatically queries AcoustID and MusicBrainz web APIs during media import to resolve canonical MusicBrainz IDs:
   - `recording_mbid` (Recording ID)
   - `release_mbid` (Album Release ID)
   - `artist_mbid` (Artist ID)
   - `release_group_mbid` (Release Group ID)
3. **Local Offline Persistence**: All resolved MusicBrainz identifiers and track relationships are saved directly into the local SQLite database (`vaino.db`). This builds a complete, offline MusicBrainz database for the local catalog, allowing the 24/7 radio engine to perform context selection and metadata display without internet access during runtime.

---

## 🎶 AudioBrainz-Inspired Audio Characteristic Descriptors

While the original AudioBrainz project is discontinued, Vaino creates and stores high-level music characteristic descriptors for each catalog track to inform intelligent selection.

```sql
CREATE TABLE track_audio_descriptors (
    track_id             TEXT PRIMARY KEY REFERENCES tracks(id),
    
    -- Acoustic & Energy Descriptors (0.0 to 1.0)
    energy               REAL,  -- Perceived intensity & activity
    valence              REAL,  -- Musical positiveness / mood (cheerful vs somber)
    danceability         REAL,  -- Rhythm regularity & beat strength
    acousticness         REAL,  -- Acoustic vs electronic synthesis balance
    instrumentalness     REAL,  -- Likelihood of pure instrumental track vs vocal
    speechiness          REAL,  -- Presence of spoken words / sound-bite nature
    
    -- Temporal & Harmonic
    tempo_bpm            REAL,  -- Beats per minute
    key_signature        TEXT,  -- Musical key (e.g., 'C Major', 'A Minor')
    loudness_lufs        REAL   -- Integrated loudness for volume normalization
);
```

---

## 🧙‍♂️ Context-Aware Auto-Playlist Selection Engine ("Singing Sorcerer")

Vaino selects the "next song" automatically by evaluating a composite score across several contextual dimensions:

```
                  +-----------------------------------+
                  |   NEXT TRACK SELECTION ENGINE     |
                  +-----------------------------------+
                                    |
     ┌───────────────┬──────────────┼──────────────┬──────────────┐
     ▼               ▼              ▼              ▼              ▼
[ Audio Descriptors ] [ Play History ] [ User Prefs ] [ Time / Day ] [ Season / Date ]
  Smooth energy      Avoid recent    Favorite       Match time      Match day of
  & tempo transitions  repeats       genres/vibes   of day energy   year context
```

### Selection Parameters
1. **Audio Descriptor Flow**: Ensures smooth transitions in energy, tempo, and mood between adjacent tracks (e.g., avoiding sudden jumps from a soft acoustic ballad to heavy electronic dance music unless requested).
2. **Play History & Frequency Rules**: Tracks played recently are penalized to ensure catalog variety and prevent repetitiveness.
3. **Listener Preferences**: Real-time weights, favorite tags, and skipped track feedback influence candidate selection.
4. **Time-of-Day Context**: Automatically shifts energy levels throughout the day (e.g., gentle/calm selections for early mornings, higher energy for afternoons, ambient/mellow for late nights).
5. **Day of Week & Day of Year (Seasonal)**: Adapts playlist vibes to weekend vs weekday schedules, as well as seasonal/holiday dates.
