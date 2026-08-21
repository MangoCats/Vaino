# GUIDE004: Android Port Strategy

**Development Guidance — how Vaino should reach a phone, and what each route costs**

Vaino selects music; it does not merely play it. A phone port therefore needs the Program Director `[SPEC009]` and `vaino.db` on the device, not a remote control for the appliance. This compares the routes.

> **Related:** [LICENSING.md](../LICENSING.md) · [GUIDE002 §2](GUIDE002-rearchitecture-plan.md#2-architectural-decisions) · [SPEC006 §6](spec/SPEC006-data-flow-and-portability.md#6-deployment-topologies) — a phone is the **Sharing** row, not a new topology

---

## 1. The licence decides more than the engineering does

**`[GDE-AND-010]` A GPL-3 fork forecloses MIT, and MIT is the whole point of Vaino.**
[LICENSING.md](../LICENSING.md) does not treat the permissive licence as a preference. It states the reason: *"Vaino has to stay permissive: it is the part people run on their own hardware, **port to their own platforms**, and embed in appliances."* An Android port is that sentence's own example.

The direction is the one already reasoned through for Sampo: **MIT may be incorporated into a copyleft work; copyleft may not be incorporated into an MIT one.** Forking a GPL-3 player makes the phone app GPL-3 — permanently, and for everyone downstream. Nothing written into that fork can come back to `player/` under MIT unless it was authored independently of the fork.

That is not an argument against GPL-3. It is an argument that this choice is **one-way**, and it is being made in the direction the project spent effort avoiding.

**`[GDE-AND-015]` The platform itself is permissive, so a ground-up build stays MIT.** Verified 2026-08-20:

| Component | Licence | MIT-compatible |
| :--- | :--- | :---: |
| Jetpack **Media3 / ExoPlayer** | **Apache-2.0** | yes |
| Jetpack **Compose**, AndroidX | Apache-2.0 | yes |
| Kotlin stdlib | Apache-2.0 | yes |
| **Auxio** | **GPL-3.0-or-later** | **no** |
| **Local Player** | MIT | yes |

Everything Google ships for audio and UI is Apache-2.0. **Only the mature players are copyleft**, and they are copyleft precisely because they are derived from other copyleft players.

---

## 2. Most of the work is the same either way

**`[GDE-AND-020]` The differentiator is smaller than it looks**, because the Vaino-specific half is identical whichever shell hosts it.

Needed regardless of route:

- **`vaino-core` extraction + JNI.** Measured 2026-08-20: the Director imports only `rusqlite`, `serde`, std collections, `crate::db` and `crate::queue::QueueEntry` — and `queue.rs` imports nothing but `VecDeque` and `PathBuf`. **No cpal, symphonia, rubato, axum or tokio.** ~3,500 lines lift cleanly.
- **Passage playback.** Vaino plays spans, not files; `MediaItem.ClippingConfiguration` is a first-class Media3 API.
- **Gain and the lead ramps.** A custom `AudioProcessor`. Media3 has no crossfade and has not for years — but `[SPEC-SC-043]` measured this library's lead-in median at **5 ms** and lead-out at **946 ms**, and says near-zero overlap is *intended*. That is a de-click envelope, not a blend.
- **Relink by content.** Phone paths resemble nothing on the Pi, and `[SPEC-RLK-025]` is the standing proof that path rewriting is not sound.
- **Bundle import**, listener-state write-back, backup, Director reload.

Provided by Media3 either way: decode, gapless, audio focus, media session, notification, media buttons, Bluetooth routing.

**So a fork saves only the shell**: library browsing, MediaStore indexing, tag parsing, playlists, search, sort, theming.

**`[GDE-AND-025]` And the shell it saves is the part Vaino replaces.** Auxio's value is *browse your library and choose*. Vaino's premise is that it chooses — the Director exists so nobody picks the next track. Their information architectures are opposed, so a fork inherits an elaborate answer to a question Vaino does not ask, then spends effort suppressing it. The db is also already authoritative for artist, album, span, gain and flavor, so MediaStore indexing is a second source of truth for facts Sampo has already established `[SPEC-DF-010]`.

---

## 3. The three routes

**`[GDE-AND-030]`**

| | Fork **Auxio** | Fork **Local Player** | **Ground-up** on Media3 |
| :--- | :--- | :--- | :--- |
| Licence outcome | **GPL-3, one-way** | MIT | **MIT** |
| Maturity inherited | high — v4.1.5, 3,427 commits | low — first release May 2026 | none |
| Shell reused | most, and mostly unwanted | some | none |
| Upstream merges | real, ongoing | light | none |
| Fights its library model | **yes** | yes | no |
| Vaino-specific work | same | same | same |

**Auxio** is the best *player* of the three and the worst *host*: its strength is the layer Vaino discards, and its licence is the one thing that cannot be undone later.

**Local Player** is licence-compatible and Compose/Media3-based, but three months old at evaluation. Forking it buys a modest head start on scaffolding against an unproven base — closer to "read it for reference" than "build on it".

**Ground-up** starts with nothing and needs nothing it cannot take under Apache-2.0.

---

## 4. Recommendation

**`[GDE-AND-040]` Build ground-up on Media3, and read the others rather than fork them.** The engineering saved by forking is the shell; the shell is largely wrong for a radio; and the price is the licence Vaino exists to keep. Reading Auxio's playback service for how it handles focus, notifications and Android's edges costs nothing and carries no obligation — **ideas are not derivative works, code is.**

**`[GDE-AND-045]` Extract `vaino-core` first, whatever is decided later.** It is worth doing on its own merits: one selection engine shared by desktop, appliance and phone, rather than a second Director in Kotlin — which is the two-implementations fault `[GDE-FBD-040]` names and the payload fixtures were built to prevent `[SPEC-SUI-130]`. It is also the honest way to find the real cost, since it is the piece no route avoids.

**`[GDE-AND-050]` The phone is a bundle target, not a new design.** `[SPEC-DF-080]`'s *Sharing* row already describes it: audio arrives with its payload, class A/B/C is imported after verification, and every advanced feature works **with no Sampo present**. The exporter and importer exist. Sampo's AGPL never approaches the phone, because the phone consumes derived data and never derives any.

---

## 5. Open

1. **`[GDE-AND-060]` Whether the phone runs the existing web UI in a WebView.** `axum` and `tokio` are Apache-2.0/MIT and the three skins already exist `[REQ-VIS-160]`. Maximum reuse, at the cost of a non-native feel and a server on a battery. Cheap to spike, and it would answer how much native UI is really wanted.
2. **`[GDE-AND-065]` Battery cost of the Director.** Measured on the appliance at 9.86 s and ~12 MB per rebuild `[IMPL-SUI-075]`; per-selection cost is unmeasured, and a phone budget is not a Pi budget.
3. **`[GDE-AND-070]` Where the library lives** — app-private storage, or a user-chosen tree via the Storage Access Framework. Affects relink, and whether other apps can see the music.

---

**Traceability:** `[GDE-AND-010..070]` · derived from [LICENSING.md](../LICENSING.md), `[GDE-ARC-018]`, `[SPEC-DF-080]`, `[GDE-FBD-040]`
