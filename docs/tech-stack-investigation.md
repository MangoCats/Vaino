# Tech Stack & Microservices Investigation

This document presents a technical analysis of system partitioning strategies, microservice architectures, candidate programming technology stacks, and acoustic feature extraction strategies for **Vaino**.

---

## 1. Microservices Partitioning Architecture

Vaino requires an architecture capable of running on two distinct host targets:
1. **Embedded Appliance**: Raspberry Pi Zero 2W (512MB total RAM, Quad-Core ARM Cortex-A53 @ 1GHz).
2. **Desktop Host**: Standard Windows, Linux, or macOS systems (plentiful CPU and memory).

### The In-Process Microservices Model
On desktop PCs, running microservices as separate system processes or Docker containers is trivial. On the Raspberry Pi Zero 2W, however, spawning multiple heavy process runtimes (e.g. separate Node or Python daemons) consumes hundreds of megabytes of RAM and risks audio stuttering.

To bridge both deployment modes, Vaino specifies a **Modular In-Process / Event-Driven Microservices Model**:
- Services are logically decoupled behind strict internal interfaces, event queues, and state contracts.
- **Embedded Deployment**: Services run as decoupled internal threads inside a single lightweight runtime process (**Modular Monolith**), consuming minimal RAM (<30MB) and overhead.
- **Desktop Deployment**: Services can optionally be spawned as independent IPC processes or microservice daemons if desired.

```
+-----------------------------------------------------------------------------------+
|                             VAINO SERVICE BOUNDARIES                              |
|                                                                                   |
|  +---------------------------+   gRPC / IPC   +--------------------------------+  |
|  |    Audio Engine Service   | <------------> | Playlist & Rec Engine Service  |  |
|  |  - High priority thread   |                | - Contextual track selector    |  |
|  |  - Slicing, crossfades    |                | - History & time-of-day scoring|  |
|  |  - Low-latency PCM stream |                +--------------------------------+  |
|  +---------------------------+                                |                   |
|                ^                                              v                   |
|                | IPC                        +----------------------------------+  |
|                v                            |     Media Catalog & Scanner      |  |
|  +---------------------------+              |   - File watcher & metadata      |  |
|  |  Web UI & API Host Service|              |   - Chromaprint / AcoustID       |  |
|  |  - REST / WebSockets API  |              |   - Essentia feature extractor   |  |
|  |  - Serves Quick/Wall UI   |              +----------------------------------+  |
|  +---------------------------+                                                    |
+-----------------------------------------------------------------------------------+
```

---

## 2. Technology Stack Evaluation

Four candidate technology stacks were evaluated against Vaino's requirements:

### Stack Option A: Rust Core + Embedded Web Server + Web Frontend (Recommended)
- **Components**: Rust core (`cpal`/`rodio` + `symphonia` for audio decoding/crossfading, `axum` for HTTP/WebSockets, `rusqlite` for DB).
- **Resource Efficiency**: **Outstanding (15–30 MB RAM total)**. Instant boot (<1 second). Statically compiled single binary with no external runtime dependencies.
- **Development & Maintenance**: Strict compiler type system prevents null pointer errors, race conditions, or audio thread deadlocks.
- **AcoustID & Feature Extraction**: Direct C/C++ FFI bindings to `libchromaprint`, `libebur128`, and `essentia`.

### Stack Option B: Go (Golang) Core Engine + Cgo Audio Analysis
- **Components**: Go (`malgo` / miniaudio bindings or `oto`/`beep`, `net/http` server, `mattn/go-sqlite3`).
- **Resource Efficiency**: **Very Good (30–60 MB RAM)**. Fast boot (<1.5 seconds). Goroutines provide simple concurrent execution for web server vs audio mixer.
- **Development & Maintenance**: High development velocity, simple syntax, easy maintenance.
- **AcoustID & Feature Extraction**: Requires Cgo wrappers or executing `fpcalc`/`ffmpeg` as external subprocesses.

### Stack Option C: Python Core (FastAPI + Sounddevice / Miniaudio + Essentia)
- **Components**: Python (`sounddevice`/`pyaudio` + `numpy`, `fastapi` + `uvicorn`, `sqlite3`).
- **Resource Efficiency**: **Poor on Embedded (150–300+ MB RAM)**. High memory footprint on RPi Zero 2W; slower boot time (4–8 seconds).
- **Development & Maintenance**: Fastest initial prototyping speed; highly accessible code.
- **AcoustID & Feature Extraction**: Native integration with `pyacoustid`, `essentia-python`, and `librosa`.

### Stack Option D: Hybrid Architecture (Rust/Go Core Player + Python Offline Scanner Worker)
- **Components**: Rust/Go core for real-time audio playback, web server, and SQLite database (~25MB RAM). Optional Python background script for offline catalog scanning and Essentia feature extraction.
- **Resource Efficiency**: **Excellent**. Keeps 24/7 audio player lightweight on RPi Zero 2W while leveraging Python's audio ML ecosystem during library imports.
- **Development & Maintenance**: Modular separation of concerns between real-time playback engine and offline analytical tools.

---

## 3. Comparison Matrix

| Evaluation Criteria | Option A: Rust | Option B: Go | Option C: Python | Option D: Hybrid (Rust + Py Scanner) |
| :--- | :--- | :--- | :--- | :--- |
| **RAM Footprint (RPi Zero 2W)** | **~15–30 MB** 🟢 | **~30–60 MB** 🟢 | **~150–300 MB** 🔴 | **~25–40 MB (Playback)** 🟢 |
| **Boot Speed (Priority 1)** | **< 1 sec** 🟢 | **< 1.5 sec** 🟢 | **4–8 sec** 🟡 | **< 1 sec** 🟢 |
| **Development Speed** | Moderate 🟡 | High 🟢 | Very High 🟢 | High 🟢 |
| **Maintenance & Safety** | Outstanding 🟢 | Very Good 🟢 | Moderate 🟡 | Good 🟢 |
| **Essentia / AcoustID FFI** | Direct C FFI 🟢 | Cgo / Subprocess 🟡 | Native Python 🟢 | Native Python (Worker) 🟢 |

---

## 4. Fingerprinting & Acoustic Feature Extraction Implementation

### Chromaprint & AcoustID (Track Identification)
- **Tooling**: `libchromaprint` C library or `fpcalc` CLI executable.
- **Workflow**:
  1. Extract a 120-second PCM audio window from the middle of a track.
  2. Feed PCM samples to Chromaprint to calculate the audio fingerprint string.
  3. Query `api.acoustid.org` web service with client API key to retrieve MusicBrainz track and album IDs.

### Essential Acoustic Features (AcousticBrainz Concept)
- **Tooling**: **Essentia** (C++ / Python library by MTG Music Technology Group - the original engine behind AcousticBrainz) and **`libebur128`**.
- **Extracted Attributes**:
  - **Loudness Normalization**: EBU R128 integrated loudness (LUFS) and loudness range (LU) to calculate volume normalization gain across crossfades.
  - **Rhythm & Dynamics**: BPM, onset detection, danceability score.
  - **Tonal Characteristics**: Key signature, scale (major/minor), chord progressions.
  - **High-Level Mood Descriptors**: Energy rating, valence/mood, acoustic vs electronic balance, vocal vs instrumental likelihood.
- **Embedded Resource Throttling**: Feature extraction involves heavy Fast Fourier Transforms (FFTs). On Raspberry Pi Zero 2W, extraction must run at lowest CPU priority (`nice +19`) sequentially during idle hours, or be pre-processed on desktop hosts during media imports.

---

## 5. Evolutionary Strategy Analysis: Rapid Option D Development → Option A Migration

A common engineering strategy is to prototype and develop rapidly using a hybrid Python-centric approach (**Option D**), then migrate the core runtime to Rust (**Option A**) for production deployment.

### 🌟 Merits (Advantages)
1. **Rapid Algorithm & Schema Validation**:
   - The database schema (track metadata, fade offsets, play history) and the context-aware recommendation math (time of day scoring, acoustic feature distance) can be designed, tested, and tweaked in Python in days without battling Rust's strict borrow checker during exploratory R&D.
2. **Instant Integration with Audio ML Tooling**:
   - Python has pre-compiled `pip install essentia pyacoustid librosa` binaries ready to use immediately. Testing feature extraction pipelines against real media collections happens instantly without managing complex C++ build toolchains or Rust FFI bindings upfront.
3. **API & Interface Contract Stabilization**:
   - The Web REST/WebSocket API endpoints and the frontend (Quick Control & Wall Art modes) can be fully realized and debugged in Python. Once these protocols are stable, porting the backend to Rust becomes a mechanical code translation rather than an open-ended design phase.

### ⚠️ Drawbacks (Risks & Costs)
1. **System Rewrite Overhead ("Pay Twice")**:
   - Implementing the real-time audio playback engine (trim slicing, dual-buffer crossfader, ALSA output) twice requires non-trivial duplicate coding effort.
2. **Audio Hardware Buffer Discrepancies**:
   - Python high-level sound wrappers (PortAudio / `sounddevice`) abstract away ring buffer management. Rust audio crates (`cpal` / `rodio`) interact more directly with ALSA / WASAPI audio device buffers, meaning audio streaming behavior learned in Python may require re-tuning in Rust.
3. **Refactoring Inertia**:
   - If the Python prototype works "well enough" on desktop PCs, there is a risk of delaying or abandoning the Rust migration, leaving the RPi Zero 2W embedded target constrained by high memory consumption (~150MB+ RAM).

### ⚖️ Net Benefit Assessment: HIGH (Recommended Evolutionary Path)

```
 [ PHASE 1: Rapid Prototyping (Option D) ]
   - Validate SQLite Schema & Web UI
   - Implement Essentia / AcoustID Pipeline in Python
   - Tune Recommendation Engine Math
                   │
                   ▼
 [ PHASE 2: Production Migration (Option A) ]
   - Port Audio Engine & Web Server to Rust (<30MB RAM)
   - Retain Python script as optional Desktop Scanner / Sidecar
```

**Conclusion**: The net benefit of this phased migration strategy is **VERY HIGH**. It de-risks the most uncertain project elements (UI responsiveness, playlist selection math, audio feature extraction) early in the project lifecycle, while ensuring the final production system achieves the instant-on, sub-30MB RAM target required for the Raspberry Pi Zero 2W.

---

## 6. Formally Selected Technology Stack Strategy

The **Option D → Option A Evolutionary Migration Strategy** has been **selected** as the official technical implementation plan for Vaino.

### Selected Phased Execution Plan

#### 🏁 Phase 1: Rapid Python/Hybrid Prototype (Option D)
- **Objective**: Rapidly build, validate, and iterate on core project features.
- **Components**:
  - **Playback & Crossfade Engine**: Python (`sounddevice` / `miniaudio` + `numpy` for fade ramps).
  - **Database & Metadata**: SQLite + `musicbrainzngs` + `pyacoustid`.
  - **Acoustic Feature Extractor**: `essentia-python` + `libebur128`.
  - **Web Control Plane**: FastAPI + WebSockets + HTML5/JS (Quick Control & Wall Art interfaces).
- **Deliverables**: Fully functioning desktop prototype, validated SQLite database schema, stabilized WebSocket API protocol, working Wall Art UI, and proven playlist auto-selection logic.

#### 🚀 Phase 2: Production Engine Migration to Rust (Option A)
- **Objective**: Achieve instant-on (<1s) boot, sub-30MB RAM footprint, zero-GC audio streaming, and fault resilience for 24/7 Raspberry Pi Zero 2W deployment.
- **Components**:
  - **Core Binary**: Rust (`symphonia` decoder + `rodio`/`cpal` audio output + `axum` web server + `rusqlite`).
  - **Desktop Scanner Sidecar**: Retain the Phase 1 Python script as an optional desktop background scanning worker for heavy Essentia batch imports.
- **Deliverables**: Production-ready, single compiled binary for Linux (ARM64/RPi Zero 2W) and Desktop (Windows/macOS/Linux).

