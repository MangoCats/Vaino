# SONOS004: What Option B (Vaino Direct to Sonos) Actually Requires

**Development Guidance — investigated on `Sonos`, 2026-08-28**

[SONOS002](SONOS002-integration-options.md) `[GDE-SONOS-180..200]` sketched Vaino-direct as SOAP calls Vaino already validated live, plus "an encoder, not a protocol." This works out what that actually means concretely, and gives the RAM question a real, if unmeasured, number.

> **Related:** [SONOS002](SONOS002-integration-options.md) `[GDE-SONOS-180..200]` · [sendspin/SPIN004](../sendspin/SPIN004-opensubsonic-deep-dive.md) `[GDE-MSA-220]` — the crossfade limitation this design avoids

---

## 1. Three pieces, one of them genuinely new

**`[GDE-SONOS-340]` The encoder is the only capability Vaino does not already have.** `PassageDecoder` and the engine's own mixer already produce f32 PCM continuously for `cpal`; Sonos's `CurrentURI` needs a widely-playable streamed format, and PCM/WAV is not it. Two realistic candidates: `mp3lame-encoder` (Rust bindings over libmp3lame — mature, small, fast) or `shine` (pure-Rust, fixed-point, no C toolchain dependency at all — the simpler cross-compile story given `build/Dockerfile.aarch64` already has to carry a C toolchain for other reasons).

**`[GDE-SONOS-350]` Tap the engine's post-mix output, not `PassageDecoder` per passage — this avoids a limitation the OpenSubsonic design accepted on purpose.** `[GDE-MSA-220]` chose to build the OpenSubsonic `stream` endpoint on a fresh `PassageDecoder` per request specifically because OpenSubsonic has no queue concept of its own to hand continuity to — and priced losing crossfade as an acceptable, permanent consequence of that. Vaino-direct has no such constraint: encoding the same mixed samples the engine already hands to `cpal` for local output means Sonos hears *exactly* what the local speaker hears, crossfades and all, with no second rendering path to keep in sync.

**`[GDE-SONOS-360]` One continuous stream, not one URL per track.** `SetAVTransportURI` is called once, pointing at a single, permanent Vaino endpoint that behaves like an internet radio station; `Play` starts it. The Director's own track changes flow through the same pipe without renegotiating UPnP per track — the same shape `node-sonos-http-api` and similar long-standing community tools already use for exactly this. Pause, stop, and volume map onto the same `AVTransport`/`RenderingControl` SOAP actions already validated live against this exact pair while writing [SONOS001](SONOS001-appliance-survey.md).

---

## 2. RAM: a reasoned estimate, explicitly not yet a measurement

**`[GDE-SONOS-370]` Every new consumer is small on its own terms:**

| Piece | Estimate |
| :--- | ---: |
| MP3 encoder instance state (LAME or `shine`) | ~100–300 KB |
| PCM tap buffer, encoder input side | ~10–30 KB |
| Encoded-output buffer feeding the HTTP response | ~30–130 KB |
| One long-lived HTTP connection (axum/tokio) | a few KB |
| **Total, steady-state, while actively streaming** | **well under 1 MB** |

**`[GDE-SONOS-380]` For scale: smaller than one passage buffer Vaino already carries.** The engine's own fixed-capacity buffer is ~5.3 MB per open passage at 44.1 kHz stereo f32 — the number this whole project is built around not exceeding, per its own "audio is never decoded whole" design principle. This feature's entire new footprint is plausibly smaller than that single existing allocation, not an additional class of memory pressure on a 512 MB appliance.

**`[GDE-SONOS-390]` Binary size grows modestly, not materially:** LAME's compiled footprint is a few hundred KB; `shine`, being pure Rust, is comparable. Neither approaches the scale of a dependency this project has already weighed carefully — `reqwest`, gated behind `sampo-support` specifically because it was not free `[GDE-MSA-250]`.

**`[GDE-SONOS-400]` This is reasoned from known library sizes, not measured on real hardware — stated as plainly as `[GDE-SONOS-290]` already flagged it.** Answering it for real means building the feature-gated route and profiling vainopi while it streams, not extrapolating further from a table.

---

## 3. Recommendation

**`[GDE-SONOS-410]` The memory case for Option B is settled enough to stop being the reason not to build it.** Whatever finally decides whether to build this, it should not be RAM — the honest estimate here is a rounding error against what the appliance already carries per open passage. `[GDE-SONOS-250]`'s original recommendation (build it regardless of how Option A turns out) stands, now with a concrete answer to the one question SONOS002 could not yet answer.

---

**Traceability:** `[GDE-SONOS-340..410]` · derived from `[GDE-SONOS-180..200]`, `[GDE-MSA-220]`, `[GDE-MSA-250]`
