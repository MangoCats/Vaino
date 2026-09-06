#!/bin/bash
# SPDX-License-Identifier: MIT
#
# The lock-in escape hatch [IMPL-BOS-160], [REQ-HW-155]. Runs at every boot,
# very early, via vaino-unlock-check.service -- does nothing unless a marker
# exists, in which case it undoes --lock-in and reboots.
#
# Installed on A. Must be in place *before* --lock-in ever runs: once A is
# the overlay's read-only lower layer, adding anything to it needs the
# overlay disabled first -- exactly what this script exists to do, so it
# cannot be its own bootstrap.
#
# Status: run for real against bose 2026-09-06, marker-to-reboot-to-cleared
# all confirmed -- but only the already-disabled-stays-disabled path,
# since bose was not yet locked in. do_overlayfs 1 returned exit 0 in that
# case (raspi-config treats "already in the target state" as success, not
# failure). The actual enabled-to-disabled transition is not yet proven;
# verifying it is the natural first thing to do right after --lock-in.
set -uo pipefail

STATE_DIR=/var/vaino/unlock
MARK_C="$STATE_DIR/request"
MARK_BOOT=/boot/firmware/unlock-request
LOG="$STATE_DIR/log"

mkdir -p "$STATE_DIR"

# Two independent locations, on purpose: C is reachable over SSH with no
# card needed at all; the boot partition is plain FAT, writable from any
# reader if SSH itself is what's broken.
if [ ! -e "$MARK_C" ] && [ ! -e "$MARK_BOOT" ]; then
    exit 0
fi

{
    echo "$(date -Is) unlock requested (C:$([ -e "$MARK_C" ] && echo yes || echo no) boot:$([ -e "$MARK_BOOT" ] && echo yes || echo no))"

    # The exact inverse of --lock-in's own call: re-enables /boot/firmware
    # writes and disables the overlay, together, effective next boot.
    raspi-config nonint do_overlayfs 1
    rc=$?
    echo "do_overlayfs 1 exit $rc"

    # B back to rw too -- a full unlock restores the whole pre-lock-in
    # state, not only A. attended-import.sh already handles a *temporary*
    # reopen for one operation; this is "give it all back."
    sed -i 's#\(LABEL=LIBRARY[[:space:]]\+/srv/library[[:space:]]\+ext4[[:space:]]\+\)ro#\1defaults#' /etc/fstab
    echo "fstab: $(grep LIBRARY /etc/fstab)"
} >> "$LOG" 2>&1

# Deleted only on success -- a failure retries on the next boot instead of
# silently giving up. Accepted risk, not engineered around: do_overlayfs is
# the identical command --lock-in itself already trusts, not a new one, so
# a structural failure here would mean --lock-in was never trustworthy
# either. A boot loop on that failure is a plausible but judged-acceptable
# cost of not building retry-limit logic for a narrow convenience feature.
if [ "$rc" = 0 ]; then
    rm -f "$MARK_C" "$MARK_BOOT"
    echo "$(date -Is) markers cleared, rebooting" >> "$LOG"
else
    echo "$(date -Is) do_overlayfs did not report success -- marker kept, will retry next boot" >> "$LOG"
fi

reboot
