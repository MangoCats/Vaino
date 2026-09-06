#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# The human-facing half of the lock-in escape hatch [IMPL-BOS-160]. Writes
# the C-side marker over SSH and reboots -- the boot-partition marker is for
# when SSH itself is what's broken, and is written by hand from a reader,
# not by this script.
#
#     bash BosePi/request-unlock.sh --check
#     bash BosePi/request-unlock.sh --go
#
# Asked twice, the same discipline every script here uses before undoing
# something deliberate: this requests undoing --lock-in, on the next two
# reboots, not immediately.
#
# Status: run for real against bose 2026-09-06 -- wrote the marker,
# rebooted, confirmed the log showed do_overlayfs 1 exit 0 and the marker
# cleared. Only exercised the already-unlocked case (bose was not yet
# --lock-in'd); the real enabled-to-disabled transition is still unproven.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
. BosePi/lib.sh

HOST="${HOST:-pi@bose}"
MODE="${1:-}"
[ "$MODE" = "--check" ] || [ "$MODE" = "--go" ] || die "usage: request-unlock.sh --check|--go"

caveat \
    "Run once for real already, but only the already-unlocked case -- see" \
    "the header. The real enabled-to-disabled transition is still unproven."

step "Preconditions"
check "bose reachable"      ssh -o ConnectTimeout=10 -o BatchMode=yes "$HOST" true
check "sudo works over SSH" ssh "$HOST" sudo -n true
check "the escape hatch is actually installed" ssh "$HOST" test -x /usr/local/sbin/vaino-unlock-check.sh

step "Plan"
say "1. write /var/vaino/unlock/request on $HOST"
say "2. reboot -- this boot notices the marker, flips do_overlayfs off, reboots again"
say "3. a second reboot lands in a writable A and B"
say "bose will be briefly unreachable through both reboots."

if [ "$MODE" != "--go" ]; then
    say ""
    say "Check only. Re-run with --go to perform this."
    exit 0
fi

run "write the marker" ssh "$HOST" "sudo mkdir -p /var/vaino/unlock && sudo touch /var/vaino/unlock/request"
run "reboot" ssh "$HOST" "sudo reboot" || true

say ""
say "Requested. bose will reboot twice on its own; check back with:"
say "  ssh $HOST findmnt -no OPTIONS /srv/library"
say "  ssh $HOST cat /var/vaino/unlock/log"
say "Log: $LOG_FILE"
