# SPEC002: Database Data Model & IPC/WebSocket Protocol Specification

**Design Specification — Tier 2**

This document specifies the relational database schema, indexing strategies, REST API contracts, and WebSocket real-time messaging protocols for **Vaino**.

---

## 1. Database Relational Schema (SQLite DDL)

```sql
-- 1. Tracks & Passages Table
CREATE TABLE IF NOT EXISTS tracks (
    id                   TEXT PRIMARY KEY,       -- SHA256 relative path digest (16 hex chars)
    file_path            TEXT NOT NULL UNIQUE,   -- Absolute OS file path
    file_format          TEXT NOT NULL,          -- 'MP3', 'FLAC', 'WAV', 'OGG', 'M4A', 'DAO'
    title                TEXT NOT NULL,
    artist               TEXT NOT NULL,
    album                TEXT,
    year                 INTEGER,
    track_number         INTEGER,
    duration_ms          INTEGER NOT NULL,
    
    -- Passage Trimming & Boundary Offsets
    start_offset_ms      INTEGER DEFAULT 0,
    end_offset_ms        INTEGER DEFAULT NULL,
    
    -- Crossfade Ramp Settings
    fade_in_ms           INTEGER DEFAULT 2000,
    fade_out_ms          INTEGER DEFAULT 3000,
    fade_ramp_profile    TEXT DEFAULT 'S_CURVE', -- 'LINEAR', 'EXPONENTIAL', 'S_CURVE'
    
    -- Metadata Identifiers
    has_cover_art        BOOLEAN DEFAULT 0,
    file_mtime           REAL DEFAULT 0,
    file_size            INTEGER DEFAULT 0,
    musicbrainz_track_id TEXT,                  -- recording_mbid
    musicbrainz_album_id TEXT,                  -- release_mbid
    
    created_at           DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 2. Audio Characteristics (AudioBrainz / Essentia Features)
CREATE TABLE IF NOT EXISTS track_audio_descriptors (
    track_id             TEXT PRIMARY KEY REFERENCES tracks(id) ON DELETE CASCADE,
    energy               REAL,  -- 0.0 to 1.0 (intensity)
    valence              REAL,  -- 0.0 to 1.0 (mood: cheerful vs somber)
    danceability         REAL,  -- 0.0 to 1.0 (beat strength)
    acousticness         REAL,  -- 0.0 to 1.0 (acoustic vs electronic)
    instrumentalness     REAL,  -- 0.0 to 1.0 (vocal vs instrumental)
    speechiness          REAL,  -- 0.0 to 1.0 (speech/soundbite likelihood)
    tempo_bpm            REAL,  -- Beats per minute
    key_signature        TEXT,  -- Key (e.g., 'C Major', 'A Minor')
    loudness_lufs        REAL   -- Integrated loudness (LUFS) for volume leveling
);

-- 3. Play History & Cooldown Tracking
CREATE TABLE IF NOT EXISTS play_history (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id             TEXT REFERENCES tracks(id),
    played_at            DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed            BOOLEAN DEFAULT 1
);

-- Indexes for Fast Query Performance
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
CREATE INDEX IF NOT EXISTS idx_history_played_at ON play_history(played_at);
```

---

## 2. WebSocket Real-Time Protocol Specification

The WebSocket server operates at endpoint `ws://<host>:<port>/ws`. All messages are JSON objects.

### 2.1 Server-to-Client Broadcast: `STATUS_UPDATE`
Broadcasted instantly whenever state, track position, or volume changes.

```json
{
  "type": "STATUS_UPDATE",
  "data": {
    "state": "PLAYING",        // "IDLE", "PLAYING", "PAUSED", "STOPPED"
    "volume": 80,               // 0 to 100
    "elapsed_ms": 42150,        // Current track playback position
    "duration_ms": 239000,      // Total track duration
    "queue_length": 14,
    "current_track": {
      "id": "07763e6dced824b7",
      "title": "Hotel California",
      "artist": "Eagles",
      "album": "Hotel California",
      "year": 1976,
      "has_cover_art": true
    }
  }
}
```

### 2.2 Client-to-Server Actions
- **Volume Change**: `{"action": "VOLUME", "volume": 75}`
- **Play / Pause / Skip**: `{"action": "PLAY"}`, `{"action": "PAUSE"}`, `{"action": "SKIP"}`

---

## 3. Unit Testing Specifications

### Test Case `UT-DB-001`: Batch Upsert Atomicity
- **Test**: Upsert a batch of 1,000 track records inside a single WAL transaction.
- **Assertion**: Transaction completes in $< 20\text{ ms}$; all 1,000 records are queryable.

### Test Case `UT-IPC-001`: WebSocket Broadcast Speed
- **Test**: Trigger a volume change via REST API.
- **Assertion**: Connected WebSocket client receives `STATUS_UPDATE` within $< 10\text{ ms}$.
