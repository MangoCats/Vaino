# SONOS006: Dynamic vs Static Linking for LAME — the Crate Already Decided

**Development Guidance — investigated on `Sonos`, 2026-08-28**

[SONOS005](SONOS005-encoder-choice.md) `[GDE-SONOS-500]` flagged LGPL linking as a real question without answering the mechanics. It turns out the mechanics answer most of the question by themselves: the Rust crate this project would actually use does not offer a dynamic-linking option at all.

> **Related:** [SONOS005](SONOS005-encoder-choice.md) `[GDE-SONOS-500]` · `player/Cargo.toml`'s `rusqlite = { features = ["bundled"] }` — the existing precedent this follows · `build/Dockerfile.aarch64`, `VainoPi/setup-vainopi.sh` — the two places any change would actually land

---

## 1. Technically: yes on the Pi, no in the crate as published

**`[GDE-SONOS-520]` Nothing about aarch64 Linux prevents dynamically linking LAME — the constraint is the Rust crate, not the hardware or OS.** `mp3lame-sys` (what `mp3lame-encoder` wraps) **vendors LAME 3.100's own C source and always statically compiles it** — read directly from its `build.rs`: on Unix it drives LAME's own `autotools` build via the `autotools` crate wrapper; on Windows it compiles the source files by hand via `cc`. Neither path calls `pkg-config` or offers a feature flag to link a system library instead. **There is no dynamic-linking option to turn on** — using this crate at all means static linking, full stop, by the crate's own unconditional design, not by any choice this project would make.

**`[GDE-SONOS-530]` Getting dynamic linking would mean abandoning the crate, not configuring it.** The only route to a dynamically-linked LAME is bypassing `mp3lame-sys`/`mp3lame-encoder` entirely and hand-writing `extern "C"` FFI bindings against a system `libmp3lame.so`, plus the link directive to find it. That is exactly the "write and maintain your own unsafe FFI layer" cost [SONOS005](SONOS005-encoder-choice.md) `[GDE-SONOS-460]` counted against `shine` — paid here instead, and for no clear gain, since LAME's own encoding API is not smaller than shine's.

---

## 2. What the static path actually needs to build

**`[GDE-SONOS-540]` Cross-compilation is handled correctly, and needs one new thing in the build image, not the appliance.** The build script detects `HOST != TARGET` and passes `--host=<target-triple>` to LAME's `configure` (with heuristic remapping for a few target triples, and an escape hatch env var if a mapping is ever wrong) — this is not a naive invocation that would silently build the wrong architecture. What it assumes, and does not bundle itself, is that **autoconf, automake, libtool, and make already exist in the build environment.**

**`[GDE-SONOS-550]` `build/Dockerfile.aarch64` does not have them today — a small, concrete, purely build-time addition:**

```
FROM rust:1.90-bookworm
RUN dpkg --add-architecture arm64 && apt-get update && apt-get install -y \
      gcc-aarch64-linux-gnu libasound2-dev:arm64 pkg-config && \
    rustup target add aarch64-unknown-linux-gnu
```

`autoconf automake libtool` (`make` is already present in the `rust:1.90-bookworm` base) would need adding to that one `apt-get install` line. This changes nothing about vainopi itself, nothing about `VainoPi/setup-vainopi.sh`, and nothing about `deploy.sh`/`deploy-player.sh`'s single-binary redeploy loop — it is entirely contained in the Docker image that only ever runs on the development machine.

---

## 3. What the dynamic path would actually need instead

**`[GDE-SONOS-560]` A real, but not unprecedented, appliance-side addition — `libasound2` already set this exact precedent.** `VainoPi/setup-vainopi.sh`'s package list already installs `libasound2` as a one-time appliance dependency, because `cpal` dynamically links ALSA — Vaino's binary is not, and has never claimed to be, a fully static artifact with zero runtime dependencies. Adding `libmp3lame0` (or whatever Debian names it) to that same list would be following an existing pattern exactly, not breaking one. **This lands in `setup-vainopi.sh` — the one-time appliance-provisioning step — never in the day-to-day `deploy.sh` redeploy loop**, so it would not compromise the "copy one file and it works" property that tooling was built around.

**`[GDE-SONOS-570]` The cost that does not have a precedent is the FFI layer itself, not the packaging.** `[GDE-SONOS-530]` already named it: hand-written bindings against LAME's C API, owned and maintained by this codebase indefinitely. Packaging-wise, dynamic linking is the easier half of this path; the actual work is writing what `mp3lame-sys` already gives you for free today.

---

## 4. LGPL, mechanically, for each path

**`[GDE-SONOS-580]` Dynamic linking is the case LGPL was written to make easy.** A separately-replaceable shared object is precisely LGPLv3 §4(d)(1)'s "suitable shared library mechanism" — a user can already, trivially, swap `libmp3lame.so` for a modified build without touching Vaino's own binary at all. No further accommodation is needed.

**`[GDE-SONOS-590]` Static linking asks for the alternative condition instead, and this project's own existing openness plausibly already satisfies it — stated as "plausibly," not as a legal conclusion.** §4(d)(0)'s alternative to a shared-library mechanism is providing the *Minimal Corresponding Source* — enough of the combined work's own material, in a form a recipient could use, to relink a modified LAME into it. Vaino's entire build system is already public — `build/Dockerfile.aarch64`, `Cargo.toml`, the documented cross-compile steps — meaning a recipient genuinely could swap LAME's vendored source and rebuild the whole binary from scratch, which accomplishes the same end a mechanical "relink" would. This reads as a good-faith fit for the license's intent. It is not the same thing as a lawyer's clearance, and is worth exactly that level of real review before this ships, not this document's own say-so.

---

## 5. Recommendation

**`[GDE-SONOS-600]` Static, via `mp3lame-encoder` exactly as published — the crate's own design, this project's existing precedent, and the deployment tooling already built all point the same way.** `rusqlite`'s `bundled` feature already made this exact choice, for the same cross-compile-robustness reason, elsewhere in this same `Cargo.toml` — extending it to LAME is consistency, not a new policy. The one real cost is one line added to `build/Dockerfile.aarch64`; the one real question left (LGPL's Minimal Corresponding Source condition) is a documentation-and-process fit, not an engineering one, and is already very plausibly met by how openly this project already ships.

**`[GDE-SONOS-610]` Dynamic linking is not "significantly preferable" on any axis except LGPL mechanics, and even there the static path is plausibly fine.** Its packaging cost is genuinely small (one precedented appliance package, per `[GDE-SONOS-560]`) — but its engineering cost is real and unshared with anything already built, undoing exactly the maintainability advantage [SONOS005](SONOS005-encoder-choice.md) found LAME's biggest edge over `shine` to be.

---

**Traceability:** `[GDE-SONOS-520..610]` · derived from `[GDE-SONOS-420..510]`
