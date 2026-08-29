# SONOS005: `mp3lame-encoder` vs `shine`, on Every Axis Asked

**Development Guidance — investigated on `Sonos`, 2026-08-28**

[SONOS004](SONOS004-direct-play-requirements.md) `[GDE-SONOS-340]` named both as realistic candidates without choosing between them. This chooses, against each axis asked rather than one overall impression.

> **Related:** [SONOS004](SONOS004-direct-play-requirements.md) `[GDE-SONOS-340..410]`

---

## 1. Sound quality — LAME, clearly

**`[GDE-SONOS-420]` Shine's own stated goal was never quality.** Its author, Gabriel Bouvigne — also a LAME developer — built it to "simplify the encoding algorithms as much as possible while retaining standard compatibility," explicitly without an advanced psychoacoustic model. LAME is the encoder every quality comparison in the format's history has measured *against*; shine was never trying to compete with it on this axis, by its own design intent, not by later neglect.

---

## 2. CPU load — a real gap, almost certainly irrelevant here

**`[GDE-SONOS-430]` shine is faster, but both are far past the point where it matters for one stream.** shine's own benchmark: 318× realtime on Apple M1 Pro against LAME's 88.7× — roughly 3.6× faster, not the 13× a forum post claimed for an unspecified older ARM board without NEON. Either number describes encoding a continuous audio stream in a small fraction of one core. Pi Zero 2W's Cortex-A53 carries a real hardware FPU — shine's *raison d'être* ("the only open source encoder that runs on fixed-point-only machines") is answering a constraint this hardware does not have. For one continuous 128–320 kbps stream, alongside everything else the engine already does in real time, this difference is very unlikely to be the one that decides anything.

---

## 3. Buffer/lag — no meaningful difference expected

**`[GDE-SONOS-440]` MP3's own frame structure sets the floor, not the encoder's internals.** A Layer III frame is 1152 samples regardless of which encoder produces it — roughly 26 ms at 44.1 kHz — and that framing, not encoder choice, is what sets the inherent latency floor. Neither candidate changes the format's own granularity; whatever additional buffering [SONOS004](SONOS004-direct-play-requirements.md) `[GDE-SONOS-370]` estimated applies about equally to either.

---

## 4. Compatibility with Sonos or similar — no expected difference

**`[GDE-SONOS-450]` MP3 is MP3 at the bitstream level; a standards-compliant frame from either encoder plays on any standards-compliant decoder, Sonos's own included.** The one documented behavioral difference found is unrelated to compatibility as such: shine encodes at the requested bitrate without narrowing the frequency range, where LAME can cut frequencies when a low bitrate is forced — a fidelity trade-off at very low bitrates, not a playback-compatibility one, and not a concern at the 128 kbps+ range this use case would actually run at.

---

## 5. Code maintainability — decisive, and not close

**`[GDE-SONOS-460]` `mp3lame-encoder` already exists as a mature Rust crate; nothing comparable exists for shine.** `mp3lame-encoder` (crates.io): **1,000,368 total downloads**, last published **2026-08-20** — eight days before this investigation — LGPL-3.0, maintained by a single active author, 12 published versions, none yanked. A search turned up **no published Rust crate binding shine at all** — only its own C library plus JS/WASM and Android bindings. Using shine from Vaino would mean writing and *maintaining* a hand-rolled `unsafe` FFI layer against a small C API, not adding a dependency. That is a real, ongoing cost this codebase would own indefinitely, not a one-time integration tax.

---

## 6. Security — LAME's real history, precisely scoped

**`[GDE-SONOS-470]` Every LAME CVE found is in code this design never calls.** CVE-2017-8419, -9410, -9411, -9412, and the related SourceForge bug reports are all in LAME's *file-reading frontend* (`frontend/get_audio.c`, WAV/AIFF header parsing, `unpack_read_samples`, input resampling) — the command-line tool's own untrusted-file-parsing path. Vaino would call `libmp3lame`'s pure encoding API directly against PCM sample buffers the engine's own mixer already produced — never reading an external file format at all, and never the vulnerable code. All of these were fixed in LAME 3.100 (2017), which is what any current build or binding uses.

**`[GDE-SONOS-480]` shine's security history is thin because scrutiny is thin, not because it is proven safer.** No CVEs specific to shine turned up in this search — a smaller, less-audited, embedded-focused project drawing far less attention than the format's reference implementation. Absence of reported vulnerabilities is not evidence of their absence, and this should not be read as a point in shine's favor.

---

## 7. Maintenance / reputation

**`[GDE-SONOS-490]` LAME is the format's own reference implementation; shine is a deliberately narrow, special-purpose sibling of it.** LAME has been the quality benchmark other encoders are measured against for over two decades, in essentially universal deployment. shine — 294 commits, 420 stars, 75 forks, 10 open issues, itself written by a former LAME developer — is real and moderately active, but its own documentation describes its origins as "1990s-era code" with "potential stability concerns," honestly acknowledged rather than hidden.

---

## 8. One consideration that favors neither: both are LGPL

**`[GDE-SONOS-500]` `mp3lame-encoder` is LGPL-3.0; shine is LGPL-2.** Whichever is chosen, statically linking an LGPL library into a single compiled appliance binary carries the same class of obligation — typically satisfied by dynamic linking, or by providing a way for a user to relink against a modified copy of the LGPL component. This is a real question for however Vaino's aarch64 build ultimately links it, worth a real look before shipping rather than assumed away — but it does not distinguish between the two candidates, since neither is more or less permissive than the other here.

---

## 9. Recommendation

**`[GDE-SONOS-510]` `mp3lame-encoder`, not close.** Every axis that showed a real difference — quality, maintainability, security-relevant scrutiny, reputation — favors it. The two axes where shine has a genuine edge — raw encode speed and FPU-less operation — answer a constraint Pi Zero 2W's Cortex-A53 does not have, for a single stream neither encoder would meaningfully strain a core over. The one cost unique to `mp3lame-encoder` — LGPL linking — is shared by shine in equal measure, not a reason to prefer the alternative.

---

**Traceability:** `[GDE-SONOS-420..510]` · derived from `[GDE-SONOS-340..410]`
