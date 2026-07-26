# User Interface & Control Model

This document specifies the user interface design, visual layout modes, and interaction patterns for **Vaino**.

---

## 🌐 Web Server Control Model

Vaino provides a single, unified web server interface that can be accessed by any web browser across desktop PCs, tablets, and smartphones.

- **Headless Server**: The Vaino daemon runs headlessly on the host hardware (embedded or desktop) and exposes an HTTP/WebSocket interface.
- **Universal Access**: Any browser connected to the local network can open the UI to monitor or shape the audio stream.
- **Synchronized State**: WebSockets ensure real-time synchronization across multiple connected client devices (e.g., changing a track on a phone updates the wall-mounted tablet instantly).

---

## 🎨 Dual Visual Modes

The UI provides two distinct operating modes tailored to different usage scenarios:

```
                      +----------------------------------+
                      |       VAINO WEB INTERFACE        |
                      +----------------------------------+
                                       |
                   ┌───────────────────┴───────────────────┐
                   ▼                                       ▼
        [ 🎛️ QUICK CONTROL MODE ]               [ 🖼️ WALL ART MODE ]
        - Queue manipulation                    - Fullscreen visual showcase
        - Manual song triggers                  - High-res album art
        - Channel / Vibe tuning                 - Clock & ambient decorations
        - Designed for Phone/Desktop            - Designed for Wall Tablets
```

---

## 🎛️ 1. Quick Control Mode

Quick Control Mode is designed for fast, tactical control when a user wants to check what is currently playing, jump to a specific track, or shape upcoming music.

### Key Capabilities
- **Now Playing Strip**: Title, artist, album, elapsed/remaining time, and active crossfade progress bar.
- **Upcoming Queue Management**: View upcoming auto-selected tracks, reorder items, remove unwanted songs, or drag-and-drop new tracks into the queue.
- **Manual Overrides & Triggers**: Instantly inject sound-bite clips, station IDs, or immediate song skips.
- **Vibe / Preference Sliders**: Real-time adjustment of recommendation weights (e.g., energy level, acoustic balance, tempo preferences).

---

## 🖼️ 2. Wall Art / Kiosk Mode

Wall Art Mode is tailored for permanent, wall-mounted tablet displays or dedicated monitors (e.g., an iPad or Android tablet mounted in a frame on a living room wall).

### Visual Layout & Aesthetics
- **High-Impact Album Art**: Large, high-resolution visual representation of the currently playing song's album artwork with subtle ambient background color extraction.
- **Upcoming Track Preview**: Elegant preview card showing the next 1–3 upcoming tracks and their scheduled play times.
- **Integrated Clock & Ambient Widgets**: Customizable clock display (digital/analog), date, weather, or subtle decorative elements.
- **Screen Saver & Burn-In Prevention**: Dynamic micro-animations and slow visual shifts to protect OLED/LCD wall displays during continuous 24/7 operation.

---

## 📱 Responsive Layout Targets

| Device Type | Primary Mode | Key Design Consideration |
| :--- | :--- | :--- |
| **Smartphone** | Quick Control | Touch-friendly vertical layout, quick queue access |
| **Desktop Web Browser** | Quick Control / Hybrid | Multi-column layout with queue, library browser, and controls |
| **Wall-Mounted Tablet** | Wall Art / Kiosk | High contrast, large typography, clock, zero-clutter fullscreen display |
