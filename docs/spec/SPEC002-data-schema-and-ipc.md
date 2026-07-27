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
    
    -- Metadata Identifiers & Uniform Sort Names
    has_cover_art        BOOLEAN DEFAULT 0,
    file_mtime           REAL DEFAULT 0,
    file_size            INTEGER DEFAULT 0,
    musicbrainz_track_id TEXT,                  -- recording_mbid
    musicbrainz_album_id TEXT,                  -- release_mbid
    artist_sort_name     TEXT,                  -- MusicBrainz sort tag or fallback
    album_sort_name      TEXT,                  -- Article-stripped album sort name
    title_sort_name      TEXT,                  -- Article-stripped track title sort name
    
    created_at           DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 2. Individual Artist Decomposition Junction Table [REQ-MB-020E]
CREATE TABLE IF NOT EXISTS track_artists (
    track_id             TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    artist_name          TEXT NOT NULL,
    artist_sort_name     TEXT NOT NULL,
    artist_mbid          TEXT,
    PRIMARY KEY (track_id, artist_name)
);

-- 3. Audio Characteristics (AudioBrainz / Essentia Features)
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

-- 4. Play History & Cooldown Tracking
CREATE TABLE IF NOT EXISTS play_history (
    id                   INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id             TEXT REFERENCES tracks(id),
    played_at            DATETIME DEFAULT CURRENT_TIMESTAMP,
    completed            BOOLEAN DEFAULT 1
);

-- 5. Persistent Queue & Player State across Restarts
CREATE TABLE IF NOT EXISTS player_queue (
    queue_order          INTEGER PRIMARY KEY AUTOINCREMENT,
    track_id             TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS player_state (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    current_track_id     TEXT REFERENCES tracks(id) ON DELETE SET NULL,
    playback_state       TEXT DEFAULT 'IDLE',
    volume               INTEGER DEFAULT 80,
    updated_at           DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for Fast Query Performance
CREATE INDEX IF NOT EXISTS idx_tracks_artist ON tracks(artist);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album);
CREATE INDEX IF NOT EXISTS idx_tracks_title ON tracks(title);
CREATE INDEX IF NOT EXISTS idx_track_artists_artist ON track_artists(artist_name);
CREATE INDEX IF NOT EXISTS idx_track_artists_sort ON track_artists(artist_sort_name);
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

## 3. REST API Endpoints Specification

All REST endpoints operate under the base URI `/api/v1`.

| Endpoint | Method | Parameters / Payload | Description |
| :--- | :--- | :--- | :--- |
| `/api/v1/status` | `GET` | None | Returns current player state, active track, volume, and queue length. |
| `/api/v1/library/tracks` | `GET` | `limit`, `offset`, `artist`, `album`, `letter`, `q` | Returns paginated track items matching optional filters. |
| `/api/v1/library/artists` | `GET` | `limit`, `offset`, `letter`, `q` | Returns paginated artist tiles with track/album counts. |
| `/api/v1/library/albums` | `GET` | `limit`, `offset`, `artist`, `letter`, `q` | Returns paginated album tiles sorted by `album_sort_name`. |
| `/api/v1/library/albums/{album_name}/tracks` | `GET` | `artist` (optional) | Returns tracks within specified album ordered by `track_number`. |
| `/api/v1/art/{track_id}` | `GET` | None | Streams binary cover art image (`image/jpeg`, `image/png`). |
| `/api/v1/lyrics/{track_id}` | `GET` | None | Returns lyrics content from `.lrc` or `.txt` file if present. |
| `/api/v1/player/play` | `POST` | None | Starts or resumes audio playback. |
| `/api/v1/player/pause` | `POST` | None | Pauses active audio playback. |
| `/api/v1/player/skip` | `POST` | None | Advances to next track in queue (subject to skip throttle). |
| `/api/v1/player/previous` | `POST` | None | Returns to previous track in playback history stack. |
| `/api/v1/player/volume` | `POST` | `{"volume": 0..100}` | Sets master audio volume percentage. |
| `/api/v1/queue` | `GET` | None | Returns active playback queue, current track, and history state. |
| `/api/v1/queue/add` | `POST` | `{"track_id": "...", "play_next": bool}` | Appends or inserts track/album into active playback queue. |
| `/api/v1/queue/move` | `POST` | `{"from_index": int, "to_index": int}` | Reorders item in queue. |
| `/api/v1/queue/remove/{index}` | `DELETE` | Path param `index` | Removes item at 0-based queue index. |
| `/api/v1/queue/clear` | `DELETE` | None | Removes all items from active queue. |

---

## 4. Unit Testing Specifications

### Test Case `UT-DB-001`: Batch Upsert Atomicity
- **Test**: Upsert a batch of 1,000 track records inside a single WAL transaction.
- **Assertion**: Transaction completes in $< 20\text{ ms}$; all 1,000 records are queryable.

### Test Case `UT-IPC-001`: WebSocket Broadcast Speed
- **Test**: Trigger a volume change via REST API.
- **Assertion**: Connected WebSocket client receives `STATUS_UPDATE` within $< 10\text{ ms}$.
