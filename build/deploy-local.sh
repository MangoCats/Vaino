#!/bin/bash
# Stop the locally-running Vaino, rebuild it, and relaunch it with the same
# invocation -- the single most frequently repeated step of this project's
# own development loop, previously three manual steps and a Windows-specific
# gotcha (a locked .exe, and a globally-set CC pointed at MinGW) rediscovered
# by hand each time [see build/README.md and HOWTO.md #2 for the underlying
# `env -u CC` story].
#
# Run from anywhere; the repository root is found from this script's own
# location, the same way build/verify-targets.sh already does.
#
#     build/deploy-local.sh                        # data/vaino_new.db, port 5720
#     build/deploy-local.sh mylib.db --port 6000    # anything else, passed straight through
#
# set -u only, not -e: `taskkill` legitimately exits non-zero when nothing
# was running, which is success here, not a failure to propagate.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BIN="$ROOT/player/target/release/vaino.exe"

die() { echo "deploy-local: $*" >&2; exit 1; }

# A default invocation when none is given; anything the caller does pass is
# used verbatim instead -- the same shape `VainoPi/deploy-player.sh` already
# uses for its own $HOST default.
if [ "$#" -eq 0 ]; then
    set -- "$ROOT/data/vaino_new.db" --port "${VAINO_PORT:-5720}"
fi

# The port to verify against below -- whatever follows --port in the args
# actually used above, or the same 5720 default if none was named.
PORT="${VAINO_PORT:-5720}"
prev=""
for arg in "$@"; do
    [ "$prev" = "--port" ] && PORT="$arg"
    prev="$arg"
done

echo "deploy-local: stopping any running vaino.exe ..."
taskkill //IM vaino.exe //F >/dev/null 2>&1 || true
# Windows needs a moment to actually release the file handle after taskkill
# returns -- rebuilding immediately hits "Access is denied" removing the old
# .exe often enough to be worth one short, bounded retry below rather than a
# fixed sleep guessed once and never revisited.
sleep 1

echo "deploy-local: building (env -u CC, --features sampo-support) ..."
built=0
for _ in 1 2 3; do
    if ( cd "$ROOT/player" && env -u CC cargo build --release --features sampo-support ); then
        built=1
        break
    fi
    echo "deploy-local: build failed (binary likely still locked) -- retrying ..." >&2
    sleep 2
done
[ "$built" = 1 ] || die "build did not succeed after retries"
[ -f "$BIN" ] || die "build succeeded but $BIN is missing"

LOG="$ROOT/player/target/release/vaino-local.log"
echo "deploy-local: launching -- log at $LOG"
( cd "$ROOT" && nohup "$BIN" "$@" >"$LOG" 2>&1 & )

# Polled, not a fixed wait -- the same reasoning `deploy-player.sh` already
# gives for not guessing a sleep duration against a program director whose
# own startup time scales with library size.
DEADLINE=${VAINO_DEPLOY_WAIT:-30}
got=""
for _ in $(seq 1 "$DEADLINE"); do
    got=$(curl -s --max-time 2 "http://localhost:$PORT/build" 2>/dev/null)
    [ -n "$got" ] && break
    sleep 1
done
[ -n "$got" ] || die "new process did not answer on port $PORT within ${DEADLINE}s -- see $LOG"

head_sha=$(cd "$ROOT" && git rev-parse --short HEAD 2>/dev/null || echo "unknown")
echo "deploy-local: running -- $got"
case "$got" in
    *"$head_sha"*) echo "deploy-local: matches HEAD ($head_sha)" ;;
    *) echo "deploy-local: WARNING -- reported build does not mention HEAD ($head_sha); check for uncommitted changes" >&2 ;;
esac
