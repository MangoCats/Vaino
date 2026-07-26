# Vaino Development Roadmap

This document outlines the end-to-end evolutionary development plan for **Vaino**, progressing from a basic web-controlled audio player to a fully autonomous, 24/7 continuous radio station player for desktop PCs and embedded Raspberry Pi Zero 2W targets.

---

## 🧭 Roadmap Overview & Strategy

Vaino follows the **Phased Evolutionary Strategy**:
- **Phases 1–5 (Phase 1 Strategy: Option D)**: Developed in **Python** for rapid R&D, fast schema iteration, immediate access to Essentia/AcoustID ML tools, and Web UI protocol stabilization.
- **Phase 6 (Phase 2 Strategy: Option A)**: Porting the real-time Audio Engine, Crossfader, and Web Server to **Rust** for <30MB RAM footprint, instant boot (<1s), and zero-GC 24/7 reliability on the Raspberry Pi Zero 2W.
- **Phase 7**: Advanced radio station features (News/Weather TTS, station IDs, scrobbling).

```
[ Phase 1: Basic Web-Controlled Player ]
                    │
                    ▼
[ Phase 2: Passage Trimming, DAO Slicing & Dual-Buffer Crossfading ]
                    │
                    ▼
[ Phase 3: Web UI Polish & Wall Art / Kiosk Mode ]
                    │
                    ▼
[ Phase 4: Media Import, AcoustID Fingerprinting & Essentia Extraction ]
                    │
                    ▼
[ Phase 5: Program Director & Intelligent Auto-Playlist Engine ]
                    │
                    ▼
[ Phase 6: Production Rust Engine Migration & RPi Zero 2W Appliance Deployment ]
                    │
                    ▼
[ Phase 7: Advanced Radio Features (News/Weather TTS, Jingle Injector, Scrobbling) ]
```

---

## 🎛️ Phase 1: Core Audio Engine & Basic Web Control (MVP)

> **Detailed Specification**: See **[Phase 1 Implementation Plan & Deliverables](file:///c:/Users/Mango%20Cat/Dev/Vaino/docs/phase1-plan.md)**.

* **Goal**: Establish a functional local audio player controlled via a basic web interface.
* **Target Platforms**: Desktop PC (Windows / Linux / macOS).

### Key Features
- **Local Audio Playback**: Basic decoding and playback of standard single-track files (FLAC, MP3, WAV, OGG).
- **Control API**: REST API endpoints for Play, Pause, Resume, Skip, and Volume control.
- **Basic Web UI**: Simple, responsive HTML/JS web control panel for local and network browser access.
- **Minimal Library Database**: SQLite schema storing local music file paths, track titles, artists, and durations.
- **Playback Logging**: Basic record keeping of played tracks and timestamps.

### Verification Milestone
- User can start the server, open a browser on a phone or PC, view the local library, play/pause a track, and hear audio output from the server's sound hardware.

---

## 🎵 Phase 2: Passage Trimming, DAO Slicing & Dual-Buffer Crossfading

* **Goal**: Enable continuous, gapless audio playback with custom trim points and dynamic crossfading.
* **Target Platforms**: Desktop PC.

### Key Features
- **Passage Trimming**: Support database-defined `start_offset_ms` and `end_offset_ms` to trim leading/trailing silence or play specific segments.
- **Disc-At-Once (DAO) Slicing**: On-the-fly extraction of individual songs from single long album capture files using database passage offsets.
- **Dual-Buffer Crossfader**: Active mixing engine blending the fading track out while fading the upcoming track in across configurable crossfade windows (e.g., 2–8 seconds).
- **Ramp Curves**: Support linear, exponential, and S-curve volume transition profiles.
- **Auto-Replenishing Queue**: Background queue manager maintaining at least N tracks or M minutes of queued audio ahead of playback.
- **Sound-Bite Clip Injection**: Support inserting brief audio clips (station IDs, jingles, soundbites) between songs.

### Verification Milestone
- Two consecutive tracks crossfade seamlessly into each other without gaps or volume dips; a long DAO file successfully plays as separate trimmed track passages.

---

## 🎨 Phase 3: Web UI Polish & Wall Art / Kiosk Mode

* **Goal**: Build a multi-client, production-ready web interface featuring real-time WebSocket state synchronization.
* **Target Platforms**: Desktop PC, Mobile Browsers, Wall-Mounted Tablets.

### Key Features
- **Quick Control Mode**: High-density interface for phone/tablet/desktop browsers featuring queue reordering, track skipping, and vibe tuning sliders.
- **Wall Art / Kiosk Mode**: Fullscreen ambient visual layout showcasing:
  - High-resolution album artwork with dynamic background color extraction.
  - Upcoming queue preview cards.
  - Integrated digital/analog clock and subtle decorative widgets.
  - OLED/LCD burn-in prevention micro-animations.
- **Real-Time Multi-Client Sync**: WebSocket state broadcasting keeping all connected client views synchronized within 100ms.
- **Skip Throttling**: Multi-user coordination ensuring only one track skip request is processed per 5-second window.
- **Lyrics Display**: Display synchronized or plain text lyrics alongside current playback when available in metadata.

### Verification Milestone
- Opening Wall Art Mode on a wall-mounted tablet displays album art and clock, while skipping a track from a smartphone updates the wall tablet display instantly (<100ms).

---

## 🗄️ Phase 4: Media Import, AcoustID Fingerprinting & Automated MusicBrainz Database Development

* **Goal**: Automated media catalog ingest, song boundary detection, Chromaprint/AcoustID fingerprinting, and local MusicBrainz identifier database construction.
* **Target Platforms**: Desktop PC (Full Import Mode).

### Key Features
- **Automated Directory Watcher**: Detects new or modified audio files added to local music folders.
- **Chromaprint Audio Fingerprinting (`fpcalc`)**:
  - Calculates Chromaprint fingerprints for single-track files.
  - Slices Disc-At-Once (DAO) continuous album files into song passages and calculates fingerprints per passage.
- **Automated AcoustID & MusicBrainz Identifier Database Construction**:
  - Automatically queries the **AcoustID API** using Chromaprint fingerprints to map local audio files/passages to candidate MusicBrainz Recording IDs (`recording_mbid`).
  - Queries **MusicBrainz API** to resolve and link canonical identifiers:
    - `recording_mbid` (Recording ID)
    - `release_mbid` (Album Release ID)
    - `artist_mbid` (Artist ID)
    - `release_group_mbid` (Album Release Group ID)
    - Track positions, official release dates, genres, and artist relationship links.
  - **Local Offline Persistence**: Saves all MusicBrainz IDs, track relationships, and metadata directly into the local SQLite database (`vaino.db`), constructing a complete offline MusicBrainz catalog for the local file library.
- **Essentia Acoustic Feature Extractor**: Analyzes raw audio files to compute high-level musical descriptors:
  - **Loudness**: EBU R128 integrated loudness (LUFS) for automatic gain normalization.
  - **Rhythm**: BPM, onset rate, danceability score.
  - **Harmonics**: Key signature, scale (major/minor).
  - **Mood & Timbre**: Energy, valence/mood, acousticness, instrumentalness.
- **Database Snapshot Export**: Exports static, pre-analyzed SQLite database snapshots containing all MusicBrainz mappings and audio descriptors for deployment to offline Raspberry Pi Zero 2W devices.

### Verification Milestone
- Scanning a folder of un-tagged single audio files and DAO album captures automatically populates `vaino.db` with validated MusicBrainz IDs (`recording_mbid`, `release_mbid`, `artist_mbid`), canonical track metadata, LUFS loudness values, BPM, and mood descriptors without manual tagging.

---

## 🧙‍♂️ Phase 5: Program Director & Intelligent Auto-Playlist Engine

* **Goal**: Fully autonomous, continuous radio-style playlist generation based on contextual rules and acoustic harmony.
* **Target Platforms**: Desktop PC.

### Key Features
- **Context-Aware Selection Engine ("Singing Sorcerer")**:
  - **Acoustic Transition Scoring**: Evaluates candidate tracks for smooth energy, tempo, and mood transitions.
  - **Time-of-Day Profiles**: Automatic energy curve adjustments (e.g., calm morning tracks, energetic afternoon flows, ambient late-night tracks).
  - **Day-of-Week & Seasonal Context**: Custom vibe weighting for weekends and holiday/seasonal dates.
  - **Anti-Repetition Cooldowns**: Enforces configurable penalties to prevent recent songs, albums, or artists from playing too frequently.
  - **User Preference Weights**: Integrates listener like/dislike signals to shape candidate selection probabilities.
- **Fallback Queue Safety**: Fallback logic ensuring continuous playback even if library filters yield narrow candidate pools.

### Verification Milestone
- The player operates continuously for 24 hours without user intervention, automatically shifting music styles appropriately between morning, afternoon, and night while avoiding recent track repeats.

---

## ⚡ Phase 6: Production Rust Engine Migration & RPi Zero 2W Appliance Deployment

* **Goal**: Achieve instant-on (<1s) boot, sub-30MB RAM footprint, zero-GC audio reliability, and 3-partition fault tolerance on the Raspberry Pi Zero 2W.
* **Target Platforms**: Raspberry Pi Zero 2W (Primary Appliance), Desktop PC (Native Binary).

### Key Features
- **Rust Core Migration**: Port real-time audio playback engine, trim slicer, crossfader, and HTTP/WebSocket web server to **Rust** (`symphonia` + `rodio`/`cpal` + `axum`).
- **"Eternal Sage" Fast-Boot Priority**:
  - **Priority 1**: Audio engine initializes sound hardware and starts track 1 playback immediately upon boot.
  - **Priority 2**: Web server and network stack initialize asynchronously in the background.
- **3-Partition Storage Architecture**:
  - `Partition 1 (OS)`: Immutable Read-Only OS.
  - `Partition 2 (Music)`: Read-Only media storage during operation.
  - `Partition 3 (State & DB)`: Crash-resilient Read-Write state storage with SQLite WAL mode.
- **Desktop Scanner Sidecar**: Retains the Phase 4 Python scanner script as an optional desktop background scanning worker for heavy Essentia imports.

### Verification Milestone
- Powering on a Raspberry Pi Zero 2W plays audio through its DAC HAT within seconds of power-on under 30MB RAM usage; pulling the power plug mid-song causes zero database or filesystem corruption upon reboot.

---

## 📻 Phase 7: Advanced Station Features & Extensibility

* **Goal**: Provide a full FM-radio station experience with scheduled station IDs, news/weather, and home automation integrations.
* **Target Platforms**: Embedded Appliance & Desktop Host.

### Key Features
- **News & Weather TTS Integration**:
  - Fetches news headlines via news APIs and local weather reports.
  - Generates Text-to-Speech (TTS) voice announcements played at top-of-the-hour marks between tracks.
- **Station Jingle & Ident Injector**: Timed insertion of station IDs, voice sweeps, and sound-bite clips over track fade-ins/outs.
- **ListenBrainz Scrobbling**: Automatic scrobbling of played songs to ListenBrainz social profiles.
- **MQTT / Smart Home Control**: Exposes MQTT endpoints for integration with Home Assistant (e.g., ducking audio volume during doorbell rings or starting morning station playback via automation).

---

## 📊 Phase & Feature Summary Matrix

| Phase | Milestone Name | Tech Stack | Primary Focus | Target Hardware |
| :--- | :--- | :--- | :--- | :--- |
| **Phase 1** | Core Audio Engine & Basic Web | Python (FastAPI + Sounddevice) | Basic playback, web controls, SQLite | Desktop PC |
| **Phase 2** | Trimming, DAO & Crossfading | Python + NumPy | Gapless stream, DAO slicing, crossfades | Desktop PC |
| **Phase 3** | Web UI & Wall Art Mode | Python + WebSockets + HTML/JS | Quick Control, Wall Art mode, 100ms sync | Phone, Tablet, PC |
| **Phase 4** | Media Import & Feature Extract | Python + Essentia + AcoustID | Library scan, MBIDs, LUFS, BPM, mood | Desktop PC |
| **Phase 5** | Intelligent Auto-Playlist Engine | Python (Program Director Engine) | Time-of-day scoring, vibe transitions | Desktop PC |
| **Phase 6** | Production Rust Engine & RPi | **Rust Core** (`symphonia`+`axum`) | <30MB RAM, <1s boot, 3-partition RPi | RPi Zero 2W & PC |
| **Phase 7** | Advanced Radio & Integrations | Rust Core + Extensions | News/Weather TTS, MQTT, ListenBrainz | RPi Zero 2W & PC |
