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

# An overlay-rooted appliance needs the binary written TWICE: once to the live
# overlay so the running service picks it up now, and once to the real lower
# filesystem, or the deploy evaporates at the next reboot `[IMPL-BOS-185]`.
# Every bose deploy between 2026-09-06 and this fix was RAM-only for exactly
# this reason, and reported success every time, because the check below asks
# the RUNNING process -- which was faithfully running the ephemeral copy.
# vainopi has a plain rw root and takes none of this path.
LOWER=""
if [ "$(ssh "$HOST" "findmnt -no FSTYPE /" 2>/dev/null)" = "overlay" ]; then
    LOWER=$(ssh "$HOST" "findmnt -no OPTIONS / | tr ',' '\n' | sed -n 's/^lowerdir=//p'" 2>/dev/null)
    [ -n "$LOWER" ] || die "overlay root on $HOST but no lowerdir found -- refusing to deploy blind"
    echo "deploy: $HOST has an overlay root; will persist through $LOWER"
fi

LOCAL_SUM=$(md5sum "$BIN" | cut -d' ' -f1)
REMOTE_SUM=$(ssh "$HOST" "md5sum $REMOTE 2>/dev/null | cut -d' ' -f1")
# On an overlay host the live copy is evidence of nothing durable, so the
# persisted copy is checked too and BOTH must match before this is a no-op.
PERSIST_SUM=""
[ -n "$LOWER" ] && PERSIST_SUM=$(ssh "$HOST" "sudo md5sum $LOWER$REMOTE 2>/dev/null | cut -d' ' -f1")
if [ "$LOCAL_SUM" = "$REMOTE_SUM" ] && { [ -z "$LOWER" ] || [ "$LOCAL_SUM" = "$PERSIST_SUM" ]; }; then
    echo "deploy: already running this build ($LOCAL_SUM)"
    exit 0
fi
if [ -n "$LOWER" ] && [ "$LOCAL_SUM" = "$REMOTE_SUM" ]; then
    echo "deploy: live copy is current but persisted copy is ${PERSIST_SUM:-absent} -- repairing"
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
             sudo /usr/sbin/setcap 'cap_net_bind_service=+ep' $REMOTE;
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
    # Persist ONLY now, never before. A binary that never answered must not
    # become the one the appliance boots into, so the lower layer keeps the
    # last build that actually started until this line is reached.
    if [ -n "$LOWER" ]; then
        ssh "$HOST" "sudo mount -o remount,rw $LOWER \
            && sudo install -m 755 /tmp/vaino.new $LOWER$REMOTE \
            && sudo /usr/sbin/setcap 'cap_net_bind_service=+ep' $LOWER$REMOTE \
            && sudo sync" || die "could not write $LOWER$REMOTE -- this deploy is RAM-only"
        PERSIST_SUM=$(ssh "$HOST" "sudo md5sum $LOWER$REMOTE 2>/dev/null | cut -d' ' -f1")
        [ "$PERSIST_SUM" = "$LOCAL_SUM" ] \
            || die "persisted copy is ${PERSIST_SUM:-absent}, expected $LOCAL_SUM"
        echo "deploy: persisted to $LOWER$REMOTE ($PERSIST_SUM)"
        # Best effort. The overlay holds its own lower layer, so this can
        # legitimately return EBUSY -- say so rather than implying it is ro.
        if ssh "$HOST" "sudo mount -o remount,ro $LOWER" 2>/dev/null; then
            echo "deploy: $LOWER returned to read-only"
        else
            echo "deploy: WARNING -- $LOWER left read-write; it returns to ro on the next reboot" >&2
        fi
    fi
    ssh "$HOST" "systemctl is-active vaino; journalctl -u vaino -n 3 --no-pager | tail -3"
    exit 0
fi

echo "deploy: new build did not answer (reopen-output -> $CODE); rolling back" >&2
ssh "$HOST" "sudo systemctl stop vaino;
             sudo install -m 755 ${REMOTE}.prev $REMOTE;
             sudo /usr/sbin/setcap 'cap_net_bind_service=+ep' $REMOTE;
             sudo systemctl start vaino"
die "rolled back to the previous binary"
