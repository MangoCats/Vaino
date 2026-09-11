#!/bin/bash
# Cross-compile the player for an APPLIANCE and install it there -- the
# two steps of build/README.md's own cross-compile story, run one after the
# other instead of retyped by hand each time.
#
#     build/deploy-appliance.sh            # defaults to pi@vainopi
#     build/deploy-appliance.sh pi@other-host
#
# Everything past the cross-compile step is build/install-player.sh itself,
# unchanged -- this only builds the binary it expects to already be there.
#
# Named for what it acts on, not for one machine `[GDE-DEP-030]`: it was
# `deploy-vainopi.sh` while vainopi was the only appliance, and kept that name
# when bose was added -- so nothing ever asked whether the second appliance
# behaved like the first. It does not `[IMPL-BOS-185]`.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOST="${1:-pi@vainopi}"
# Two forms of the same path: POSIX for the shell, drive-letter for docker
# -v -- the identical split build/verify-targets.sh already needs, and gets
# wrong if combined into one command substitution with ||.
DROOT=$(cd "$ROOT" && pwd -W 2>/dev/null) || DROOT=$ROOT
[ -n "$DROOT" ] || DROOT=$ROOT

die() { echo "deploy-appliance: $*" >&2; exit 1; }

echo "deploy-appliance: building the cross-compile image (cached if unchanged) ..."
docker build -q -t vaino-aarch64 -f "$ROOT/build/Dockerfile.aarch64" "$ROOT" >/dev/null \
    || die "docker build failed"

echo "deploy-appliance: cross-compiling for aarch64 ..."
MSYS_NO_PATHCONV=1 docker run --rm -v "$DROOT":/w -w /w vaino-aarch64 \
    cargo build --release --target aarch64-unknown-linux-gnu \
        --manifest-path player/Cargo.toml --features sampo-support \
    || die "cross-compile failed"

echo "deploy-appliance: installing on $HOST ..."
"$ROOT/build/install-player.sh" "$HOST"
