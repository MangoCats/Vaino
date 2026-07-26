# Vaino Phase 1 Development Plan & Deliverables

This document details the concrete implementation plan, module architecture, component design, and deliverables for **Phase 1: Core Audio Engine & Basic Web Control (MVP)** of the Vaino project.

---

## 🎯 Phase 1 Objective

Build a working, standalone Python MVP player running on desktop PC (Windows/Linux/macOS) that:
1. Scans a music folder (such as `c:\Users\Mango Cat\Music`) and populates a local SQLite database with track metadata and album artwork.
2. Plays audio files to local system sound hardware with basic Play, Pause, Resume, Skip, and Volume controls.
3. Host a FastAPI web server exposing a REST API and real-time WebSocket state updates.
4. Serves a clean, responsive single-page web UI accessible via desktop or mobile browsers.

---

## 📂 Sample Library Context (`c:\Users\Mango Cat\Music`)

Inspection of the sample audio library reveals a rich, real-world catalog structure containing:
- **Individual Track Folders**: e.g. `c:\Users\Mango Cat\Music\Eagles\Hotel_California\` containing distinct track files (`(Eagles)Hotel_California-01-Hotel_California.mp3`, `(Eagles)Hotel_California-02-New_Kid_In_Town.mp3`, etc.) alongside cover art (`cover.jpg`, `Hotelcalifornia.jpg`).
- **Disc-At-Once (DAO) Album Files**: e.g. `c:\Users\Mango Cat\Music\Eagles\Desperado.mp3` (~52MB single file containing an entire album).
- **Multiple Formats**: `.mp3`, `.flac`, `.wav`, `.ogg`, `.m4a`.

*In Phase 1, the scanner and player will index both single tracks and full album files. Offset slicing for DAO files will be fully enabled in Phase 2.*

---

## 🏛️ Phase 1 System Architecture

```
+-------------------------------------------------------------------------------+
|                            VAINO PHASE 1 RUNTIME                              |
|                                                                               |
|  +------------------------+             +----------------------------------+  |
|  |     SQLite Database    | <---------- |       Media Library Scanner      |  |
|  |       (vaino.db)       |             |     (mutagen metadata reader)    |  |
|  +------------------------+             +----------------------------------+  |
|               |                                                               |
|               v                                                               |
|  +------------------------+             +----------------------------------+  |
|  |  Audio Playback Engine | ----------> |   FastAPI Web & WebSocket Server |  |
|  | (sounddevice / miniaudio)|           |          (Uvicorn host)          |  |
|  +------------------------+             +----------------------------------+  |
|               |                                          ^                    |
|               v                                          | (HTTP/WebSocket)   |
|     SYSTEM SOUND OUTPUT                                  v                    |
|    (Speakers / Soundcard)                     +----------------------------+  |
|                                               |    Web UI Control Panel    |  |
|                                               |  (HTML5 / CSS3 / JS SPA)   |  |
|                                               +----------------------------+  |
+-------------------------------------------------------------------------------+
```

---

## 🛠️ Component Breakdown & Module Specifications

### 1. Database & Schema (`src/db/`)
- **Schema (`schema.sql`)**:
  ```sql
  CREATE TABLE IF NOT EXISTS tracks (
      id TEXT PRIMARY KEY,
      file_path TEXT NOT NULL UNIQUE,
      file_format TEXT NOT NULL,
      title TEXT NOT NULL,
      artist TEXT NOT NULL,
      album TEXT,
      year INTEGER,
      track_number INTEGER,
      duration_ms INTEGER NOT NULL,
      has_cover_art BOOLEAN DEFAULT 0,
      created_at DATETIME DEFAULT CURRENT_TIMESTAMP
  );

  CREATE TABLE IF NOT EXISTS play_history (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      track_id TEXT REFERENCES tracks(id),
      played_at DATETIME DEFAULT CURRENT_TIMESTAMP,
      completed BOOLEAN DEFAULT 1
  );
  ```
- **Scanner (`src/db/scanner.py`)**: Uses `mutagen` to scan directories, extract ID3/FLAC metadata tags, locate artwork (`cover.jpg`, embedded APIC frames), and insert/update entries in `vaino.db`.

### 2. Audio Playback Engine (`src/audio/`)
- **Engine (`src/audio/engine.py`)**:
  - Uses `miniaudio` or `sounddevice` + `pydub`/`numpy` to stream PCM audio to local speakers.
  - Runs a dedicated playback thread to prevent blocking web requests.
  - Maintains state: `IDLE`, `PLAYING`, `PAUSED`, `STOPPED`.
  - Methods: `load_track(track_id)`, `play()`, `pause()`, `resume()`, `stop()`, `skip()`, `set_volume(0..100)`, `get_status()`.

### 3. Web Server & Control API (`src/server/`)
- **FastAPI Application (`src/server/app.py`)**:
  - `GET /api/v1/status` — Returns JSON with current track info, elapsed time, total duration, volume, and playback state.
  - `POST /api/v1/player/play` — Play or resume track.
  - `POST /api/v1/player/pause` — Pause playback.
  - `POST /api/v1/player/skip` — Skip to next track in queue/library.
  - `POST /api/v1/player/volume` — Set volume (`{"volume": 75}`).
  - `GET /api/v1/library/tracks` — Returns paginated track list.
  - `GET /api/v1/art/{track_id}` — Serves binary cover art image.
  - `WS /ws` — WebSocket broadcasting JSON state changes whenever playback status updates.

### 4. Web Control Panel UI (`src/web/`)
- **Frontend SPA (`src/web/index.html`, `style.css`, `app.js`)**:
  - Modern, dark-mode visual interface.
  - **Now Playing Display**: Large album artwork, title, artist, album, animated progress bar, and time counters.
  - **Control Bar**: Play/Pause, Skip, and Volume slider.
  - **Library Table**: Filterable list of indexed tracks with one-click "Play Now" action buttons.
  - Real-time WebSocket connection to keep the UI synced with server state.

---

## 📦 Deliverables & File Layout

When Phase 1 development completes, the project directory will contain the following structure:

```
Vaino/
├── main.py                     # Entry point (CLI launcher)
├── requirements.txt            # Python dependencies (fastapi, uvicorn, sounddevice, mutagen, etc.)
├── config.json                 # Default configuration (music dir, DB path, HTTP port)
├── docs/                       # Project documentation
│   ├── phase1-plan.md          # [THIS DOCUMENT]
│   ├── roadmap.md              # 7-Phase development roadmap
│   ├── architecture.md         # System architecture
│   ├── tech-stack-investigation.md
│   ├── embedded-hardware.md
│   ├── user-interface.md
│   └── audio-database.md
└── src/                        # Source Code
    ├── audio/                  # Audio playback subsystem
    │   ├── __init__.py
    │   └── engine.py           # Sounddevice / miniaudio playback core
    ├── db/                     # Database & Scanner subsystem
    │   ├── __init__.py
    │   ├── database.py         # SQLite connection & queries
    │   ├── scanner.py          # Mutagen directory metadata scanner
    │   └── schema.sql          # DB schema definition
    ├── server/                 # Web server & API subsystem
    │   ├── __init__.py
    │   ├── app.py              # FastAPI app & routing
    │   └── websocket.py        # WebSocket manager
    └── web/                    # Static Web UI frontend
        ├── index.html          # Control panel HTML
        ├── style.css           # Premium dark-mode styling
        └── app.js              # WebSocket & API client JS
```

---

## ✅ Phase 1 Verification & Acceptance Criteria

1. **Scanner Verification**: Running `python main.py --scan "c:\Users\Mango Cat\Music"` successfully indexes tracks into `vaino.db` and extracts cover art.
2. **Audio Output Verification**: Triggering play outputs clear, distortion-free audio through desktop speakers/headphones.
3. **Web API Verification**: `GET /api/v1/status` returns correct track metadata and position; `POST /api/v1/player/pause` instantly pauses audio output.
4. **Web UI Verification**: Opening `http://localhost:8000` in a browser displays the Now Playing card with album artwork, responsive transport controls (Play/Pause/Skip), and a functional volume slider.
5. **WebSocket Verification**: Changing volume or skipping tracks in one browser tab updates all other connected browser tabs instantly.

---

## 🔍 Phase 1 Readiness Review & Resolved Technical Choices

Review of the Phase 1 specification confirms **high readiness**. Below are 5 potential implementation ambiguities and their resolved solutions:

| Potential Ambiguity | Analysis | Resolved Choice |
| :--- | :--- | :--- |
| **1. Audio Engine Library** | `sounddevice` vs `miniaudio` vs `pygame` | **`miniaudio` + `pydub`**: `miniaudio` provides pure Python wheel bindings for low-latency PCM playback across Windows, Linux, and macOS without requiring external C library setup. |
| **2. DB Forward Compatibility** | Schema support for future Phase 2 passage slicing | **Include Offset Fields Now**: `tracks` table includes `start_offset_ms DEFAULT 0` and `end_offset_ms DEFAULT NULL` in Phase 1, making Phase 2 passage trimming 100% backward-compatible without DB migrations. |
| **3. Initial Queue Behavior** | What plays when `main.py` launches? | **Auto-Populated Queue in Paused State**: On launch, the engine populates an initial playback queue from `vaino.db` and initializes in `IDLE`/`PAUSED` state awaiting Web UI / API play command. |
| **4. DAO Album Files (50MB+ MP3s)** | How are long album captures handled in Phase 1? | **Indexed as Full-Length Tracks**: Files like `Desperado.mp3` are indexed as single full-length tracks in Phase 1, ensuring immediate playability before Phase 2 passage slicing. |
| **5. Cover Art Retrieval** | Embedded tags vs folder files (`cover.jpg`) | **Fallback Priority Chain**: 1) Extract embedded APIC / METADATA picture tags. 2) Fallback to folder artwork (`cover.jpg`, `folder.jpg`, `front.jpg`). Serve via `/api/v1/art/{track_id}`. |

