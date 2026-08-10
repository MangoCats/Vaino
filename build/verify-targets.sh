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
( cd "$ROOT/player" && cargo test --release 2>&1 | grep -E "^test result: ok\.|FAILED" | head -1 ) || fail=$((fail+1))

echo
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
