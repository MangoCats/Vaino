# REQ001: System Requirements & Verification Matrix

**Authoritative Specification — Tier 1**

This document defines the formal system requirements for **Vaino**, establishing unique requirement IDs, quantitative constraints, acceptance criteria, and verification test methods for both the Python reference implementation and the Rust production migration.

---

## 1. Requirement Enumeration & Mapping

| Requirement ID | Domain | Summary | Verification Method |
| :--- | :--- | :--- | :--- |
| **`[REQ-SYS-010]`** | System | Standalone continuous radio playback server with local sound output | Automated Closed-Loop Test |
| **`[REQ-SYS-020]`** | Architecture | Strict co-located audio output (server location = audio output location) | System Integration Test |
| **`[REQ-AUD-010]`** | Audio Engine | Gapless PCM streaming from MP3, FLAC, WAV, OGG, and M4A files | Unit & Integration Test |
| **`[REQ-AUD-020]`** | Audio Engine | Passage trimming via `start_offset_ms` and `end_offset_ms` timestamp bounds | Audio Engine Unit Test |
| **`[REQ-AUD-030]`** | Audio Engine | On-the-fly Disc-At-Once (DAO) album capture slicing | Unit Test |
| **`[REQ-AUD-040]`** | Audio Engine | Dual-buffer crossfader with Linear, Exponential, and S-Curve ramp profiles | PCM Buffer Unit Test |
| **`[REQ-AUD-050]`** | Audio Engine | Dynamic Master Volume scaling (0–100%) without clipping | PCM Buffer Unit Test |
| **`[REQ-DB-010]`** | Database | SQLite embedded database with WAL mode and transaction batching | DB Benchmark Test |
| **`[REQ-DB-020]`** | Database | Fast incremental scanning using `file_mtime` and `file_size` (<0.1s re-scan) | Benchmark Test |
| **`[REQ-MB-010]`** | Metadata | Chromaprint (`fpcalc`) audio fingerprinting & AcoustID lookup | API Integration Test |
| **`[REQ-MB-020]`** | Metadata | Automated local MusicBrainz identifier database construction (`recording_mbid`) | DB Schema Test |
| **`[REQ-FE-010]`** | Feature Extract | Essentia acoustic feature extraction (LUFS loudness, BPM, key, valence, energy) | Unit Test |
| **`[REQ-FE-020]`** | Feature Extract | 3-Tier Acoustic Resolver (mulib.db ground truth, AcousticBrainz API, MusicNN ONNX) | Unit & Integration Test |
| **`[REQ-PD-010]`** | Selection | Autonomous context-aware playlist selection ("Singing Sorcerer") | Recommendation Test |
| **`[REQ-PD-020]`** | Selection | Anti-repetition cooldown penalty enforcement for recent tracks/artists | Algorithm Test |
| **`[REQ-PD-030]`** | Selection | Time-of-Day Programs & Reference Preference Track Slots | Unit & Integration Test |
| **`[REQ-PD-040]`** | Selection | Logarithmic restraint preference factor tuning ($10^{-\text{restraint}}$ multiplier) | Unit & API Test |
| **`[REQ-PD-050]`** | Selection | Exponential rotation hard lockout & linear recovery ramp window enforcement | Unit & Integration Test |
| **`[REQ-PD-060]`** | Selection | Occasion & calendar seasonal weighting (`[C]`, `[W]`, `[S]`, `[K]`) | Unit Test |
| **`[REQ-PD-070]`** | Selection | Multi-stage candidate pool refining & roulette-wheel weighted random sampling | Unit & Simulation Test |
| **`[REQ-PD-080]`** | Selection | Persistent play tracking (`play_count` and `last_played_at` timestamps) | DB & Integration Test |
| **`[REQ-UI-010]`** | Interface | Embedded HTTP REST API & WebSocket real-time state broadcast (<100ms sync) | WebSocket Test |
| **`[REQ-UI-020]`** | Interface | Quick Control SPA & Fullscreen Wall Art Mode with album art and clock | E2E Browser Test |
| **`[REQ-UI-030]`** | Interface | Interactive track & artist preference editor UI (Rotation, Recovery, Restraint sliders) | E2E Browser Test |
| **`[REQ-HW-010]`** | Embedded Target | Raspberry Pi Zero 2W target with <30MB RAM footprint & <1s instant boot | HW Resource Test |
| **`[REQ-HW-020]`** | Storage | Fault-tolerant 3-partition storage architecture (Immutable OS, RO Media, RW DB) | Power Interruption Test |

---

## 2. Detailed Functional Requirements

### 2.1 Audio Engine & Pipeline
- **`[REQ-AUD-010]` Audio Format Decoding**: The audio decoder MUST decode single-track audio files and long capture files in MP3, FLAC, WAV, Vorbis OGG, and M4A formats into 32-bit floating-point PCM audio arrays normalized between `-1.0` and `+1.0`.
- **`[REQ-AUD-010B]` Exact PCM Decoded Duration & Database Caching**: When loading an audio track for playback, `AudioEngine` MUST compute exact track duration from total PCM sample frame counts, overriding inaccurate VBR MP3 header estimations. The corrected exact duration MUST be cached back into `vaino.db` so future library queries serve exact runtimes instantly without requiring re-decoding or CPU load.
- **`[REQ-AUD-020]` Passage Trimming**: Given a track record with `start_offset_ms` $t_{start}$ and `end_offset_ms` $t_{end}$, the playback engine MUST begin audio decoding at $t_{start}$ and emit a track transition event at $t_{end}$.
- **`[REQ-AUD-030]` DAO Multi-Song Passage Slicing & Tracklist Suppression**: The system MUST detect continuous Disc-At-Once (DAO) album capture files, query MusicBrainz for official release tracklists, and generate bounded passage records in `vaino.db` (`start_offset_ms`, `end_offset_ms`, `track_number`, `title`, `musicbrainz_track_id`, `musicbrainz_album_id`). Playing a passage track MUST stream ONLY its designated sub-section ($t_{\text{start}}$ to $t_{\text{end}}$). When sliced passage tracks exist for an album, `get_album_tracks` MUST automatically suppress the raw unsliced continuous file, displaying only the individual passage tracks ordered by `track_number ASC, start_offset_ms ASC`.
- **`[REQ-AUD-040]` Crossfade Mixing Math**: When transitioning between Track $A$ and Track $B$ over a crossfade window $T_x$ seconds:
  $$\text{Out}(t) = \text{Track}_A(t) \cdot (1 - \alpha(t)) + \text{Track}_B(t) \cdot \alpha(t)$$
  where $\alpha(t) \in [0.0, 1.0]$ is governed by the configured ramp curve profile (`LINEAR`, `EXPONENTIAL`, or `S_CURVE`).

### 2.2 Metadata & MusicBrainz Identifier Database
- **`[REQ-MB-010]` Chromaprint Fingerprinting**: The catalog scanner MUST compute a Chromaprint fingerprint from a 120-second PCM slice of each audio track/passage using `libchromaprint` or `fpcalc`.
- **`[REQ-MB-020A]` MusicBrainz ID Linkage**: The scanner MUST query AcoustID and MusicBrainz APIs to populate `recording_mbid` and `release_mbid` into the local `tracks` database table.
- **`[REQ-MB-020B]` MBID Trust Hierarchy**: The system MUST treat AcoustID fingerprint-verified MBIDs in `vaino.db` as the authoritative source of truth, overriding raw embedded MP3 tags.
- **`[REQ-MB-020C]` Album Cover Art Resolution & SQLite Caching**: The system MUST resolve high-resolution album cover art using a prioritized fallback pipeline: (1) SQLite `album_cover_art` table cache, (2) Local folder cover image files (`cover.jpg`, `folder.jpg`, `album.jpg`), (3) MusicBrainz Recording/Release MBID lookup via Cover Art Archive (`coverartarchive.org`), and (4) MusicBrainz Release Search API. All fetched artwork MUST be stored in `album_cover_art` for persistent offline serving via `GET /api/v1/art/{track_id}`. All MusicBrainz/CAA network requests MUST enforce a 1.25s rate-limit delay and HTTP 503/429 exponential backoff retries.
- **`[REQ-MB-020D]` MusicBrainz Artist Sort Name Keying**: The system MUST store canonical MusicBrainz `artist_sort_name` (e.g., `Springsteen, Bruce`, `Beatles, The`, `Eagles, The`) in `vaino.db` and use it as the key for Alpha-Artist letter bar filtering (`[REQ-UI-020E]`), while maintaining standard human-friendly artist display names (`Bruce Springsteen`, `The Beatles`) in the UI.
- **`[REQ-MB-020E]` Individual Artist Decomposition**: The scanner MUST decompose multi-artist strings (e.g. `Santana feat. Rob Thomas`, `B.B. King / Eric Clapton`) into individual canonical artist records stored in the `track_artists` relational junction table.
- **`[REQ-UI-020G]` Individual Artist Tile Relabeling & Portfolio Browsing**: In Artists View, each artist tile MUST be labeled with the exact individual artist name it represents (e.g., `Sarah McLachlan`, `Cyndi Lauper` rather than `Sarah McLachlan & Cyndi Lauper`). Selecting an artist tile MUST display a list of ALL albums featuring that artist on any track (including solo, duet, featured, and collaboration tracks).

### 2.3 User Interface & Hierarchical Navigation
- **`[REQ-UI-020A]` Hierarchical Navigation Views**: The web interface MUST provide dedicated human-friendly navigation views: Browse by Artist, Browse by Album, and Album Tracklist View.
- **`[REQ-UI-020B]` Album Track Number Ordering**: Within an Album view, available tracks MUST be sorted strictly by `track_number`. Album cover art MUST be displayed prominently in album and tracklist views.
- **`[REQ-UI-020C]` Smart Artist Drilldown & Auto-Bypass**: When selecting an Artist:
  - If the artist has $\ge 2$ albums in the library, display a subset album selection screen containing only that artist's albums.
  - If the artist has exactly $1$ album in the library, automatically bypass the single-item album screen and navigate directly to that album's sorted tracklist.
- **`[REQ-UI-020D]` Breadcrumb Filter Navigation & View Reduction**: Selecting an artist MUST filter album tiles to show ONLY albums by that artist. Selecting an album MUST reduce the display to a list of ONLY tracks on that particular album. Active filters MUST be indicated with a 1-click removable breadcrumb tag (`[ 🎙️ Artist: Eagles ✖ ]`).
- **`[REQ-UI-020E]` A–Z Letter Prefix Filter**: The A–Z navigation bar MUST filter items by prefix matching on computed sort names: `artist_sort_name` in Artists View, `album_sort_name` in Albums View, and `title_sort_name` in Tracks View. When no entity-specific filter is active (e.g., browsing all albums without an artist filter), the letter bar MAY additionally match across related sort fields. Selecting `ALL` clears the letter prefix filter. The `#` filter MUST match items whose sort name begins with a digit (`GLOB '[0-9]*'`).
- **`[REQ-UI-020F]` Alpha Filter Stacking & Auto-Reset**: Selecting a letter while filtered by artist MUST stack to show ONLY albums/tracks by that artist starting with that letter. Selecting an artist or album card MUST automatically reset the alpha filter to `ALL` upon opening the target view, while allowing subsequent letter selection within that subset.
- **`[REQ-UI-020H]` Full Album Context & Non-Sticky Top-Tab Navigation**: Selecting an album card (e.g. *I Am Sam*) MUST display ALL available tracks on that album, clearing any active artist filter. Clicking top-level view tabs (**Tracks**, **Artists**, **Albums**) MUST clear stale breadcrumbs to restore clean un-filtered top-level catalog views.
- **`[REQ-UI-020I]` Uniform Sort Name Rules, Leading Special Character Stripping & Uppercase Transformation**: All navigable entity types — Artists, Albums, and Tracks — MUST apply uniform sort name computation and diacritic normalization. Specifically: (1) leading whitespace, quotes (`' " \``), brackets (`( ) [ ] { }`), and punctuation (`, .`) MUST be stripped from the front of the name string, (2) leading English article stripping per `[REQ-UI-020J]`, (3) diacritic/accent normalization (e.g., `Mötley Crüe` → `Motley Crue`, `Beyoncé` → `Beyonce`) via Unicode NFD decomposition with combining mark removal, and (4) transformation to ALL CAPS (`UPPER()`) so capitalization has no impact on sort order or letter bar filtering (`[REQ-UI-020E]`). Sort name columns MUST be stored in `vaino.db` and populated during library scanning.
- **`[REQ-UI-020J]` Sort Name Computation & Article Stripping Fallback**: When an entity (artist, album, or track) lacks an explicit MusicBrainz sort tag, the system MUST compute a sort name by: (1) stripping leading special characters per `[REQ-UI-020I]`, (2) stripping leading English articles (`The `, `A `, `An `) and appending them after a comma (e.g., `The Dark Side of the Moon` → `Dark Side of the Moon, The`; `An Evening With...` → `Evening With..., An`), (3) applying diacritic normalization, and (4) converting the result to uppercase. Non-article words MUST NOT be rearranged algorithmically (e.g., `Simple Minds` remains `SIMPLE MINDS` under `S`, not `MINDS, SIMPLE`). Names already containing a comma (e.g., `Springsteen, Bruce`) MUST be preserved in `Surname, Given` format after special character stripping, diacritic normalization, and uppercase conversion.
- **`[REQ-UI-020K]` Dynamic Pagination Controls & Page Size Dropdown**: All views (Tracks, Artists, Albums) MUST provide a page size selector with options `10`, `25`, `50` (default), `100`, and `250`. When total items exceed page size, navigation controls (`First`, `Prev`, `Next`, `Last`) MUST be dynamically shown. When total items fit on a single page, navigation buttons MUST be hidden while keeping the page size dropdown available for adjustment.
- **`[REQ-UI-030]` Interactive Track & Artist Preference Editor UI**: The Web UI MUST provide interactive rating and preference dialogs for individual tracks and artists, featuring slider controls for **Rotation**, **Recovery**, **Restraint**, and **Profanity** with live human-readable time conversion labels (e.g., `15min`, `6.0hrs`, `3.2day`), displaying `play_count` and `last_played_at` statistics.

### 2.4 Playlist Queue & Track History Management
- **`[REQ-QUE-010]` Queue Data Structure & Prioritization**: The audio engine MUST maintain an ordered FIFO playlist queue. User-enqueued tracks (single or album) MUST take immediate priority over auto-generated Program Director selections.
- **`[REQ-QUE-020]` Enqueue Operations (Single Track & Album Batch)**: The REST API and Web UI MUST support enqueuing individual tracks or entire albums (sorted by `track_number`) with options to **Play Next** (insert at index 0 of queue) or **Add to End** (append to queue).
- **`[REQ-QUE-030]` Interactive Queue Manipulation**: Users MUST be able to inspect the full queue, remove items at any index, reorder tracks (move up/down), and clear the queue.
- **`[REQ-QUE-040]` Track History Memory & Transport Controls**: The audio engine MUST maintain a `history_stack` of played tracks. The UI transport bar MUST provide **`⏮ Previous`**, **`▶/⏸ Play/Pause`**, **`⏭ Next`**, and **`≡ Queue`** toggle controls.

### 2.5 Program Director & Selection Algorithm
- **`[REQ-PD-010]` Candidate Scoring & Multi-Pass Selection Engine**: The next song selection engine MUST evaluate candidate tracks using composite multiplicative weighting combined with multi-pass candidate pool refining and roulette-wheel weighted random sampling:
  $$W(k) = W_{\text{restraint, trk}} \cdot W_{\text{restraint, art}} \cdot W_{\text{rec, trk}} \cdot W_{\text{rec, art}} \cdot W_{\text{rec, rel}} \cdot W_{\text{occasion}} \cdot W_{\text{flow}} \cdot \sqrt{\frac{180}{\text{duration\_sec}}}$$
- **`[REQ-PD-020]` Exponential Rotation Lockout & Recovery Window Ramp**:
  - The selection engine MUST calculate rotation lockout duration $T_{\text{rot}} = 10^{\text{rotation}} \cdot 3600\text{s}$ and recovery duration $T_{\text{rec}} = 10^{\text{recovery}} \cdot 3600\text{s}$ for candidates, artists, and related tracks.
  - If time elapsed since last play $\Delta t < T_{\text{rot}}$, candidate weight MUST be forced strictly to `0.0` (hard rotation lockout).
  - If $T_{\text{rot}} \le \Delta t < T_{\text{rot}} + T_{\text{rec}}$, candidate weight MUST be scaled by linear recovery ramp fraction $\frac{\Delta t - T_{\text{rot}}}{T_{\text{rec}}}$.
- **`[REQ-PD-030]` Time-of-Day Programs & Musical Preference Slots**: The system MUST persist time-slot programs (`programs` table in `vaino.db`) defining start time boundaries (`start_time` in `HH:MM` format) and reference seed track IDs. `ProgramDirector` MUST match the active program closest to current clock time in the past (with 24-hour midnight wrap-around) and compute the mean 11D target preference vector across assigned seed tracks. The REST API (`/api/v1/programs`) and Web UI MUST provide interactive controls for managing time slot boundaries and reference seed track lists.
- **`[REQ-PD-040]` Logarithmic Restraint Preference Factor**: Continuous user preference scaling MUST apply logarithmic multiplier $W_{\text{restraint}} = 10^{-\text{restraint}}$, where restraint values range from $-1.000$ (frequency boost) to $+3.335$ (near total silence).
- **`[REQ-PD-050]` Occasion & Calendar Seasonal Weighting**: The selection engine MUST calculate date-sensitive occasion multipliers for tagged tracks (`[C]` Christmas, `[W]` Winter, `[S]` Summer, `[K]` Kids), dynamically scaling weights based on current month and day.
- **`[REQ-PD-060]` Persistent Play Statistics**: Upon track playout completion, the engine MUST increment persistent `play_count` and update `last_played_at` timestamps for both the track and its associated artist(s) in `vaino.db`.
- **`[REQ-PD-070]` Multi-Stage Candidate Pool Refining & Roulette-Wheel Sampling**:
  1. *Eligibility Filtering*: Remove tracks with total calculated weight $W(k) < 0.001$.
  2. *Program Seed Alignment*: Down-select overall pool to target size $N_{\text{excl}}$ (default 1000) by removing tracks farthest in 11D AcousticBrainz vector space from active program seed tracks.
  3. *Queue-Tail Transition Re-sorting*: Re-sort candidate pool by 11D acoustic similarity to the last track in the active queue to optimize playback transition flow.
  4. *Roulette-Wheel Weighted Selection*: Draw next track using fitness-proportionate random sampling with position decay ($0.96^i$).
- **`[REQ-FE-020]` 3-Tier Acoustic Resolver Fallback Strategy**: The system MUST resolve 11-dimensional AcousticBrainz feature descriptors (`ab_acoustic` through `ab_tonal`) using a strict 3-tier fallback hierarchy:
  1. **Tier 1 (Ground Truth Transfer)**: Direct transfer from local `mulib.db` ground truth database using exact recording MBID or relative file path matching ($R = 1.000$).
  2. **Tier 2 (Online API Lookup)**: HTTP REST query to the official AcousticBrainz online repository (`acousticbrainz.org/api/v1/{mbid}/low-level`) when recording MBID is present.
  3. **Tier 3 (Local Deep Net Inference)**: Local inference via MTG Essentia `msd-musicnn-1.onnx` 200-D feature embedding backbone and 10 pre-trained binary classification heads (acoustic, aggressive, danceable, gender, happy, instrumental, party, relaxed, sad, tonal) when un-indexed.

---

## 3. Non-Functional & Performance Constraints

### 3.1 Hardware Resource Constraints (Raspberry Pi Zero 2W)
- **`[REQ-HW-010A]` Maximum Memory Footprint**: The production Rust runtime (Phase 6) MUST NOT consume more than **30.0 MB** of RSS RAM during continuous playback.
- **`[REQ-HW-010B]` Boot Readiness Priority**: Track 1 audio playback MUST begin within **1.0 second** of core daemon startup. HTTP/WebSocket management services MUST initialize asynchronously without delaying audio playback.

### 3.2 Real-Time WebSocket Synchronization & Remote Accessibility
- **`[REQ-UI-010A]` Broadcast Latency**: All connected WebSocket clients MUST receive state broadcast updates within **100 milliseconds** of a state change event (Play, Pause, Skip, Volume, Track Change).
- **`[REQ-UI-010B]` Configurable Multi-User Skip Throttling**: The server MUST enforce a configurable minimum throttle window (default: `5.0` seconds, configurable via `skip_throttle_seconds` in `config.json`) between user skip commands across all connected web clients. Both the REST `POST /api/v1/player/skip` endpoint and the WebSocket `SKIP` action MUST respect this global throttle. When a skip is throttled, the server MUST return the current status without advancing the track.
- **`[REQ-UI-010C]` Keyboard Transport Shortcuts & Auto-Reconnect**: The web interface MUST support global keyboard transport controls (`Space` for Play/Pause, `Right Arrow` for Skip, `Left Arrow` for Skip Back) and automatic WebSocket reconnection with exponential backoff.
- **`[REQ-DB-010B]` Filename Metadata Extraction Fallback**: When audio files lack valid ID3v2/FLAC tags, the scanner MUST parse the filename structure (e.g. `01 - Hotel California.mp3`) to extract track number and title fallback fields.
