#!/bin/bash
# Put a freshly cross-compiled player on the appliance, and prove it took.
#
# Run from the repository root on the development machine:
#
#     VainoPi/deploy-player.sh [host]
#
# Idempotent: running it twice with the same binary is a no-op after the first,
# because it compares checksums before doing anything.
#
# The reason this is a script rather than three ssh commands: an evening was
# spent reading a 404 as a routing defect when the truth was that the Pi was
# running a binary older than the source that defined the route. Deploying
# without checking what is now running invites exactly that, so the last thing
# this does is ask the running process to identify itself, and put the old
# binary back if it cannot `[SPEC-APS-140]`.
set -uo pipefail

HOST="${1:-pi@vainopi}"
PORT="${VAINO_PORT:-5720}"
BIN=player/target/aarch64-unknown-linux-gnu/release/vaino
REMOTE=/usr/local/bin/vaino

die() { echo "deploy: $*" >&2; exit 1; }

[ -f "$BIN" ] || die "no binary at $BIN -- cross-compile first (see build/README.md)"

# Refuse to ship the wrong architecture. Cheap, and the failure it prevents is
# a service that will not start on a machine that is now unreachable by design.
case "$(file -b "$BIN" 2>/dev/null)" in
    *aarch64*) ;;
    *) die "$BIN is not an aarch64 binary" ;;
esac

ssh -o ConnectTimeout=10 "$HOST" true 2>/dev/null \
    || die "$HOST is not reachable"

LOCAL_SUM=$(md5sum "$BIN" | cut -d' ' -f1)
REMOTE_SUM=$(ssh "$HOST" "md5sum $REMOTE 2>/dev/null | cut -d' ' -f1")
if [ "$LOCAL_SUM" = "$REMOTE_SUM" ]; then
    echo "deploy: already running this build ($LOCAL_SUM)"
    exit 0
fi
echo "deploy: $REMOTE_SUM -> $LOCAL_SUM"

scp -q "$BIN" "$HOST:/tmp/vaino.new" || die "upload failed"
ssh "$HOST" "md5sum /tmp/vaino.new | grep -q $LOCAL_SUM" \
    || die "uploaded binary does not match; not installing"

# Keep the outgoing binary. A player that will not start leaves an appliance
# with no web interface, which is also the only way back into it.
ssh "$HOST" "sudo cp -f $REMOTE ${REMOTE}.prev 2>/dev/null;
             sudo systemctl stop vaino;
             sudo install -m 755 /tmp/vaino.new $REMOTE;
             sudo systemctl start vaino" || die "install failed"

# Ask the RUNNING process what it is. A 404 here means an older binary is
# serving, whatever the checksum on disk says.
#
# POLLED, not a fixed wait. This was `sleep 8`, which was true when the
# appliance held a 31-file test library and false the moment it held the real
# one: the Program Director is built at startup and takes 9.86 s over 8,330
# passages [SPEC-RLK-075 measures the sibling case], so the web server binds at
# about 15 s. The check therefore began failing a good binary and rolling it
# back, reporting "did not answer" -- which invites diagnosing the build rather
# than the deadline. A number tuned against small data that silently rots as
# the data grows is the same shape of fault as the quadratic browse in
# [REQ-LIB-165].
DEADLINE=${VAINO_DEPLOY_WAIT:-90}
CODE=000
for _ in $(seq 1 "$DEADLINE"); do
    CODE=$(ssh "$HOST" "curl -s -o /dev/null -w '%{http_code}' --max-time 3 -X POST \
            http://localhost:$PORT/command/reopen-output" 2>/dev/null)
    [ "$CODE" = "204" ] && break
    sleep 1
done
if [ "$CODE" = "204" ]; then
    echo "deploy: running, and answering as the new build"
    ssh "$HOST" "systemctl is-active vaino; journalctl -u vaino -n 3 --no-pager | tail -3"
    exit 0
fi

echo "deploy: new build did not answer (reopen-output -> $CODE); rolling back" >&2
ssh "$HOST" "sudo systemctl stop vaino;
             sudo install -m 755 ${REMOTE}.prev $REMOTE;
             sudo systemctl start vaino"
die "rolled back to the previous binary"
