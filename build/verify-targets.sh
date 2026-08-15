#!/bin/sh
# Run the player's test suite on every supported target.
#
# Written after an audit found that "cross-compiles cleanly" had been standing
# in for "works": the suite had only ever RUN on Windows, aarch64 was compiled
# but never executed, and Linux x86_64 had never been built at all. Compiling is
# not testing.
#
# It also found a bug that only a real audio device could expose -- the Windows
# device opens at 48 kHz against a 44.1 kHz library, and the resampler was not
# wired into the playback path, so audio ran 8.8% fast. A null sink reports no
# rate, so it hid the fault entirely. Hence [D] below.
#
#   A  Linux x86_64            native in Docker
#   B  Linux aarch64           cross-compiled, executed under emulation
#   C  Windows x86_64          run on the host
#   D  real audio device       manual; a null sink cannot catch rate mismatches
#
# Usage:  sh build/verify-targets.sh
set -u
# Two forms of the same path: POSIX for the shell, native for docker -v.
# Combining them with || in one command substitution ran BOTH branches.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
DROOT=$(cd "$ROOT" && pwd -W 2>/dev/null) || DROOT=$ROOT
[ -n "$DROOT" ] || DROOT=$ROOT
fail=0

echo "== A: Linux x86_64 =="
docker build -q -t vaino-linux -f "$ROOT/build/Dockerfile.linux" "$ROOT" >/dev/null || fail=$((fail+1))
MSYS_NO_PATHCONV=1 docker run --rm -v "$DROOT":/w -w /w vaino-linux \
    cargo test --release --manifest-path player/Cargo.toml --target-dir /tmp/t \
    2>&1 | grep -E "^test result: ok\.|FAILED" || fail=$((fail+1))

echo "== B: Linux aarch64 (cross-compiled, run under emulation) =="
docker build -q -t vaino-aarch64 -f "$ROOT/build/Dockerfile.aarch64" "$ROOT" >/dev/null || fail=$((fail+1))
MSYS_NO_PATHCONV=1 docker run --rm -v "$DROOT":/w -w /w vaino-aarch64 \
    cargo test --release --no-run --target aarch64-unknown-linux-gnu \
    --manifest-path player/Cargo.toml >/dev/null 2>&1 || fail=$((fail+1))
BIN=$(ls -t "$ROOT"/player/target/aarch64-unknown-linux-gnu/release/deps/vaino_player-* 2>/dev/null \
      | grep -v '\.d$' | head -1)
if [ -n "$BIN" ]; then
    REL=${BIN#"$ROOT"/}
    MSYS_NO_PATHCONV=1 docker run --rm --platform linux/arm64 -v "$DROOT":/w -w /w \
        debian:bookworm-slim sh -c \
        "apt-get update -qq >/dev/null 2>&1 && apt-get install -y -qq libasound2 >/dev/null 2>&1; ./$REL" \
        2>&1 | grep -E "^test result: ok\.|FAILED" || fail=$((fail+1))
else
    echo "  aarch64 test binary not found"; fail=$((fail+1))
fi

echo "== C: host (Windows or Linux) =="
# `env -u CC`: a globally-set CC (e.g. CC=C:\mingw64in\gcc.exe) makes the cc
# crate compile bundled SQLite with MinGW while rustc links with MSVC, which
# fails on ___chkstk_ms. Unset, the cc crate finds MSVC itself and it builds.
# Cleared here so the result does not depend on the developer's environment.
( cd "$ROOT/player" && env -u CC cargo test --release 2>&1     | grep -E "^test result: ok\.|FAILED" | head -1 ) || fail=$((fail+1))

# The bounded-decode gate. It needs a long file from a real library, which no
# build machine has by default, so it is opt-in via VAINO_LONG_FILE -- and a run
# without one reports SKIPPED rather than passing quietly. `[REQ-AUD-110]`
echo
echo "== Bounded decode (optional: set VAINO_LONG_FILE) =="
mem_note=""
if [ -n "${VAINO_LONG_FILE:-}" ]; then
    if [ -f "$VAINO_LONG_FILE" ]; then
        ( cd "$ROOT/player" && env -u CC cargo run --release --quiet --bin memcheck -- \
              "$VAINO_LONG_FILE" 2>&1 | tail -4 ) || fail=$((fail+1))
    else
        echo "  VAINO_LONG_FILE is set but does not exist: $VAINO_LONG_FILE"
        fail=$((fail+1))
    fi
else
    mem_note="bounded decode NOT checked (VAINO_LONG_FILE unset)"
    echo "  $mem_note"
fi

# The skins are HTML, CSS and JavaScript, so cargo cannot reach them. Optional
# because the player needs neither node nor jsdom to run; a skip is reported as
# a skip, never folded into the pass.
echo
echo "== Skins (optional: needs node + jsdom) =="
skins_note=""
if command -v node >/dev/null 2>&1; then
    node "$ROOT/build/verify-skins.js"
    case $? in
        0) ;;
        2) skins_note="skins NOT checked (jsdom missing)" ;;
        *) fail=$((fail+1)) ;;
    esac
else
    skins_note="skins NOT checked (node missing)"
fi

# The Python tools are outside cargo's reach too. `apply_reviews` rewrites what
# a passage IS, and shipped once in a state where it could not write at all, so
# it does not get to be untested `[REQ-LIB-165]`.
echo
echo "== Tools (optional: needs python) =="
tools_note=""
if command -v python >/dev/null 2>&1; then
    python "$ROOT/tools/test_apply_reviews.py" || fail=$((fail+1))
elif command -v python3 >/dev/null 2>&1; then
    python3 "$ROOT/tools/test_apply_reviews.py" || fail=$((fail+1))
else
    tools_note="tools NOT checked (python missing)"
fi

echo
if [ -n "$mem_note" ]; then
    echo "$mem_note"
fi
if [ -n "$tools_note" ]; then
    echo "$tools_note"
fi
if [ -n "$skins_note" ]; then
    echo "$skins_note"
fi
if [ "$fail" -eq 0 ]; then
    echo "ALL TARGETS PASS"
else
    echo "$fail target check(s) failed"
fi
echo
echo "NOT covered here -- must be run by hand on a machine with audio:"
echo "  D) play a passage through a REAL device and confirm the rate is converted."
echo "     A null sink reports no device rate and cannot catch a resampling fault."
exit "$fail"
