# GUIDE004: Phone Port Strategy

**Development Guidance — how Vaino should reach a phone, and what each route costs**

Android and iOS reach the same answer by different arguments, and the iOS one is
shorter. §§1–3 are Android; §4 is what changes on iOS.

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

## 4. iOS: the same answer, reached faster

**`[GDE-IOS-010]` The licence stops being a preference and becomes a distribution
blocker.** On Android a GPL-3 fork is shippable and merely forecloses MIT. On
iOS it is not shippable at all: the App Store's terms impose per-device and DRM
restrictions that conflict with the GPL's guarantee of free redistribution, and
this is settled by precedent rather than argument — **VLC was pulled from the
App Store in January 2011** over exactly this, and returned only after
relicensing away from pure GPL.

So the fork-or-build question does not need weighing here. It is answered by the
store's terms before engineering is consulted.

**`[GDE-IOS-015]` And there is nothing to fork in any case.** The Android survey
found a mature, well-architected candidate in Auxio — 3,427 commits. The iOS
equivalent does not exist: the open-source landscape is small demonstration
players and abandoned samples. The one mature open-source media application on
the platform is VLC, which is a video player that had to change its licence to
be there.

**`[GDE-IOS-020]` The audio stack suits Vaino better than Android's does.** This
is the one place iOS is ahead, and it is not a small lead:

| Need | Android (Media3) | iOS (AVAudioEngine) |
| :--- | :--- | :--- |
| Play a passage span | `ClippingConfiguration`, millisecond | **`scheduleSegment(_:startingFrame:frameCount:at:)` — frame-exact** |
| Lead-in / lead-out ramp | custom `AudioProcessor` | mixer-node volume ramp, native |
| Crossfade | **absent**, requested since 2017 | two player nodes into a mixer |

Vaino's passage is a frame range with a gain and two ramps `[SPEC-SC-040]`, and
`scheduleSegment` is that signature almost exactly. Sample-accurate boundaries
also matter more here than they look: `[SPEC-SA-092]` showed boundary error
propagating into *flavor*, not merely into playback.

**`[GDE-IOS-025]` Rust reaches iOS more cleanly than Android, but only from a
Mac.** `aarch64-apple-ios` plus **UniFFI** generates Swift bindings from the
Rust source, packaged as an XCFramework — no JNI boilerplate to hand-write, so
`vaino-core` `[GDE-AND-045]` binds more directly than it would on Android.

**The cost is hardware and a subscription.** Building the Rust library for iOS
*requires a macOS host* — the SDK and toolchain are macOS-only — and signing and
distribution require Xcode plus the Apple Developer Program for anything beyond
a seven-day personal provisioning profile. This project's entire toolchain today
is Windows with a Linux container for the Pi `[GDE-AND-020]`; Android needs no
new machine, and iOS needs one before the first line compiles.

**`[GDE-IOS-030]` Getting the library onto the device is the awkward part.**
There is no rsync over ssh into a sandbox. Music lands in the app's own
Documents directory through the Files app, Finder file sharing, or a transfer
the app implements itself, and no other application can see it. Two
consequences, and neither is fatal:

- **Relink is unaffected and still required** — it binds by content, and the
  device's paths will match nothing `[SPEC-RLK-030]`.
- **A subset is the normal case**, which the design already states: *"a Pi
  holding a subset of a 44 GB library is a normal deployment"* `[SPEC-RLK-060]`.
  A phone holding a subset is the same sentence.

**`[GDE-IOS-035]` Net.** Engineering favours iOS; logistics favour Android. The
audio model fits better, the Rust binding is cleaner, and the licence question
answers itself — against a Mac, a developer subscription, App Store review, and
a sandbox that makes moving 44 GB a design problem rather than a command.

---

## 5. Recommendation

**`[GDE-AND-040]` Build ground-up on both, and read the others rather than fork them.** On Android that is a judgement — the fork is possible and the shell it saves is mostly unwanted. On iOS it is not a judgement: nothing mature exists to fork, and a copyleft fork could not be distributed if it did.

On Android specifically: The engineering saved by forking is the shell; the shell is largely wrong for a radio; and the price is the licence Vaino exists to keep. Reading Auxio's playback service for how it handles focus, notifications and Android's edges costs nothing and carries no obligation — **ideas are not derivative works, code is.**

**`[GDE-AND-045]` Extract `vaino-core` first, whatever is decided later.** It is worth doing on its own merits: one selection engine shared by desktop, appliance and phone, rather than a second Director in Kotlin — which is the two-implementations fault `[GDE-FBD-040]` names and the payload fixtures were built to prevent `[SPEC-SUI-130]`. It is also the honest way to find the real cost, since it is the piece no route avoids.

**`[GDE-AND-050]` The phone is a bundle target, not a new design.** `[SPEC-DF-080]`'s *Sharing* row already describes it: audio arrives with its payload, class A/B/C is imported after verification, and every advanced feature works **with no Sampo present**. The exporter and importer exist. Sampo's AGPL never approaches the phone, because the phone consumes derived data and never derives any.

---

## 6. Open

1. **`[GDE-AND-060]` Whether the phone runs the existing web UI in a WebView.** `axum` and `tokio` are Apache-2.0/MIT and the three skins already exist `[REQ-VIS-160]`. Maximum reuse, at the cost of a non-native feel and a server on a battery. Cheap to spike, and it would answer how much native UI is really wanted.
2. **`[GDE-AND-065]` Battery cost of the Director.** Measured on the appliance at 9.86 s and ~12 MB per rebuild `[IMPL-SUI-075]`; per-selection cost is unmeasured, and a phone budget is not a Pi budget.
3. **`[GDE-AND-070]` Where the library lives** — app-private storage, or a user-chosen tree via the Storage Access Framework. Affects relink, and whether other apps can see the music.

---

**Traceability:** `[GDE-AND-010..070]`, `[GDE-IOS-010..035]` · derived from [LICENSING.md](../LICENSING.md), `[GDE-ARC-018]`, `[SPEC-DF-080]`, `[GDE-FBD-040]`
