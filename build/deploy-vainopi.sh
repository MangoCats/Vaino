#!/bin/bash
# Cross-compile the player for the appliance and put it on vainopi -- the
# two steps of build/README.md's own cross-compile story, run one after the
# other instead of retyped by hand each time.
#
#     build/deploy-vainopi.sh            # defaults to pi@vainopi
#     build/deploy-vainopi.sh pi@other-host
#
# Everything past the cross-compile step is VainoPi/deploy-player.sh itself,
# unchanged -- this only builds the binary it expects to already be there.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOST="${1:-pi@vainopi}"
# Two forms of the same path: POSIX for the shell, drive-letter for docker
# -v -- the identical split build/verify-targets.sh already needs, and gets
# wrong if combined into one command substitution with ||.
DROOT=$(cd "$ROOT" && pwd -W 2>/dev/null) || DROOT=$ROOT
[ -n "$DROOT" ] || DROOT=$ROOT

die() { echo "deploy-vainopi: $*" >&2; exit 1; }

echo "deploy-vainopi: building the cross-compile image (cached if unchanged) ..."
docker build -q -t vaino-aarch64 -f "$ROOT/build/Dockerfile.aarch64" "$ROOT" >/dev/null \
    || die "docker build failed"

echo "deploy-vainopi: cross-compiling for aarch64 ..."
MSYS_NO_PATHCONV=1 docker run --rm -v "$DROOT":/w -w /w vaino-aarch64 \
    cargo build --release --target aarch64-unknown-linux-gnu \
        --manifest-path player/Cargo.toml --features sampo-support \
    || die "cross-compile failed"

echo "deploy-vainopi: deploying to $HOST ..."
"$ROOT/VainoPi/deploy-player.sh" "$HOST"
