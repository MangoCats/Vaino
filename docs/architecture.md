# System Architecture & Audio Pipeline

This document outlines the core software architecture, audio playback engine pipeline, and system boundary rules for **Vaino**.

---

## 🏛️ High-Level System Architecture

Vaino follows a decoupled, headless core architecture where the playback engine runs locally on the host machine, serving an embedded web server that acts as the universal control plane and display layer.

```
+-----------------------------------------------------------------------+
|                              VAINO HOST                               |
|                                                                       |
|  +------------------------+          +-----------------------------+  |
|  |    Audio Database      | -------- | Playlist / Rec Engine       |  |
|  |  (SQLite / Embedded)   |          | (Context-Aware Selector)    |  |
|  +------------------------+          +-----------------------------+  |
|               |                                     |                 |
|               v                                     v                 |
|  +-----------------------------------------------------------------+  |
|  |                       AUDIO PLAYBACK ENGINE                     |  |
|  |  - Decoder (FLAC/MP3/WAV/DAO)                                   |  |
|  |  - Trimmer (Start/End Offset slicing)                          |  |
|  |  - Dual-Buffer Crossfader & Ramp Profile Mixer                  |  |
|  +-----------------------------------------------------------------+  |
|                               |                                       |
|                               v                                       |
|                  LOCAL HARDWARE AUDIO OUTPUT                          |
|             (Analog D/A HAT, Bluetooth, Soundcard)                    |
|                                                                       |
|  +-----------------------------------------------------------------+  |
|  |                       EMBEDDED WEB SERVER                       |  |
|  |  - REST / WebSocket API                                         |  |
|  |  - Serves Quick Control & Wall Art Interfaces                   |  |
|  +-----------------------------------------------------------------+  |
+-----------------------------------------------------------------------+
                                ^
                                | (HTTP / WebSockets)
                                v
           +-----------------------------------------+
           |           REMOTE CONTROL CLIENT         |
           |   (Phone, Tablet, Desktop Web Browser)  |
           +-----------------------------------------+
```

---

## 🎵 Audio Pipeline & File Model

Vaino processes audio as a continuous stream rather than discrete, isolated song files. 

### Supported Input Media Types
1. **Song-Per-File**: Standard audio tracks (FLAC, MP3, AAC, OGG, WAV).
2. **Disc-At-Once (DAO) / Long Capture Files**: Single continuous audio files representing entire albums or live broadcasts, sliced on the fly using database-defined start and end timestamp offsets.
3. **Sound-Bite Clips**: Brief station IDs, jingles, ambient snippets, or transition audio inserted dynamically between tracks.

### Processing Pipeline
Every item queued into the stream passes through the following pipeline stages:

```
[Audio Media File] 
      │
      ▼
[1. Slicing & Trimming] ──► Applies DB StartOffset & EndOffset (Crucial for DAO files)
      │
      ▼
[2. Fade-In Ramp]      ──► Applies DB FadeInTime & Ramp Profile (Linear, Exponential, S-Curve)
      │
      ▼
[3. Active Playback]   ──► Main audio body stream
      │
      ▼
[4. Dynamic Crossfade] ──► Blends with upcoming track's Fade-In Ramp over DB CrossfadeWindow
      │
      ▼
[5. Local PCM Stream]  ──► Output to system sound subsystem (ALSA / WASAPI / CoreAudio)
```

---

## 📍 Strict Audio Output Locality Rule

> **Rule**: *Where the Vaino server runs is strictly where the audio output streams.*

Vaino is not a network audio streaming server (like Icecast or AirPlay speaker output to clients). The web interface functions purely as a **control and visualization plane**. Audio is output directly from the host system's hardware sound interfaces (e.g., Raspberry Pi DAC hat, Bluetooth speaker pair, or PC soundcard).

---

## 🔄 Relationship to McRhythm

Vaino is the spiritual successor to **McRhythm**, an earlier project with similar goals:
- **Inspiration**: Inherits the vision of an automated, continuous, radio-style personal station and non-stop crossfading audio engine.
- **Independence**: Vaino is a completely new project written from scratch. It is not constrained by McRhythm’s codebase, software dependencies, or legacy design decisions.
