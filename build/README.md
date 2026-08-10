# Cross-compilation

## aarch64 (Raspberry Pi Zero 2W, 64-bit Pi OS)

```
docker build -t vaino-aarch64 -f build/Dockerfile.aarch64 .
docker run --rm -v "$(pwd)":/w vaino-aarch64 \
    cargo build --release --target aarch64-unknown-linux-gnu --manifest-path player/Cargo.toml
```

### Why a container

A bare `cargo build --target aarch64-unknown-linux-gnu` from Windows fails twice,
and the two failures are different in kind:

1. `alsa-sys` -- *"pkg-config has not been configured to support cross-compilation"*.
   `cpal` needs libasound headers and libs for the target.
2. with `cpal` removed -- *"linker `cc` not found"*.

Everything else, including symphonia, rubato, rustfft and realfft, compiles for
aarch64 with no configuration at all. Rust's ARM friction here is not Rust's: it
is the single C dependency ALSA brings in, plus a linker. The image supplies both.

Verified 2026-08-10: full symphonia + rubato + cpal stack cross-compiled in 25.8 s
to a 1.9 MB binary, and **executed under ARM emulation**, linking only
`libasound.so.2`, `libgcc_s`, `libm`, `libc` -- all present on stock Raspberry Pi OS.

### Note on the target triple

The Pi currently running MuLibPlay (`bose.lan`) is **armv7l, 32-bit**, not aarch64.
If that machine is the deployment target rather than a 64-bit Pi Zero 2W, the
triple is `armv7-unknown-linux-gnueabihf` and needs its own probe -- the same
Dockerfile pattern applies with `gcc-arm-linux-gnueabihf` and `libasound2-dev:armhf`.


## Host builds on Windows

If `CC` is set globally to a MinGW compiler, bundled SQLite is compiled with
MinGW while rustc links with MSVC, and the build fails on an unresolved
`___chkstk_ms`. With `CC` unset the `cc` crate locates MSVC itself:

```
env -u CC cargo test --release
```

`build/verify-targets.sh` clears it, so verification does not depend on the
developer's environment. Nothing is wrong with the MinGW toolchain — it is
simply the wrong compiler for an `x86_64-pc-windows-msvc` target.

## Verifying every target

```
sh build/verify-targets.sh
```

Runs the suite on Linux x86_64, Linux aarch64 (cross-compiled then executed
under emulation) and the host. It deliberately reports what it cannot cover:
playback through a real audio device. A null sink reports no device rate, so it
cannot detect a sample-rate fault -- which is exactly how unresampled playback
survived until a real device was used `[REQ-HW-147]`.
