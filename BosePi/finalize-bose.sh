#!/usr/bin/env bash
# SPDX-License-Identifier: MIT
#
# Phase 5: bring vaino and mpd up, then -- only when explicitly told the
# machine has been heard playing -- lock the card down [IMPL-BOS-120].
#
# **Status: not yet run.** Nothing in this file has executed against real
# hardware. --lock-in's `raspi-config nonint do_overlayfs 0` call was checked
# by reading bose's own raspi-config source on 2026-09-06 to confirm it both
# enables the overlay and offers to write-protect /boot/firmware in one call
# -- that is real, but the *call itself* has not been made. `nonint` is
# raspi-config's stable public interface, not an internal it's expected to
# change casually, but a future Raspberry Pi OS release restructuring it is
# exactly the kind of drift this file cannot see coming. Read this script's
# output, especially --lock-in's, rather than trusting its exit code alone --
# the point of no easy return [IMPL-BOS-120] is not a good place to discover
# a step silently did nothing.
#
#     bash BosePi/finalize-bose.sh --start
#     bash BosePi/finalize-bose.sh --lock-in --confirm-heard-it-play
#
# Two subcommands, not one script with a flag that defaults somewhere: --start
# is safe and idempotent, reversible by re-running it. --lock-in is the point
# of no easy return this project's own docs name explicitly -- after it, a
# mistake in /etc means another card swap, so it refuses to run without the
# one flag that cannot be typed by accident, and does not infer consent from
# anything else in this script's own state.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1
. BosePi/lib.sh

HOST="${HOST:-pi@bose}"
MODE="${1:-}"

case "$MODE" in
    --start)   ;;
    --lock-in) [ "${2:-}" = "--confirm-heard-it-play" ] || die \
        "refusing: --lock-in requires --confirm-heard-it-play as well." \
        "This is deliberately not inferable from any other state -- see" \
        "[IMPL-BOS-120]. Play something through bose and listen to it first." ;;
    *) die "usage: finalize-bose.sh --start | --lock-in --confirm-heard-it-play" ;;
esac

caveat \
    "This script has not itself been run before -- see the header." \
    "Watch each step below rather than trusting a clean exit at the end."

step "Preconditions"
check "bose reachable"       ssh -o ConnectTimeout=10 -o BatchMode=yes "$HOST" true
check "sudo works over SSH"  ssh "$HOST" sudo -n true
check "vaino binary present" ssh "$HOST" test -x /usr/local/bin/vaino
check "vaino.db present"     ssh "$HOST" test -f /var/vaino/vaino.db
check "mpd installed"        ssh "$HOST" command -v mpd

if [ "$MODE" = "--start" ]; then
    step "Install vaino's unit"
    scp -q BosePi/vaino-bose.service "$HOST:/tmp/vaino.service" || die "scp failed"
    run "install unit + daemon-reload" ssh "$HOST" \
        "sudo cp /tmp/vaino.service /etc/systemd/system/vaino.service && sudo systemctl daemon-reload"

    step "Start mpd and vaino (not enabled yet -- that's --lock-in's job)"
    run "start mpd"   ssh "$HOST" "sudo systemctl unmask mpd.socket 2>/dev/null; sudo systemctl start mpd"
    run "start vaino" ssh "$HOST" "sudo systemctl start vaino"

    step "Check both are actually active, not just accepted"
    check "mpd is active"   ssh "$HOST" systemctl is-active --quiet mpd
    check "vaino is active" ssh "$HOST" systemctl is-active --quiet vaino
    say "Both running. Play something through bose before running --lock-in."
    exit 0
fi

# --lock-in from here.
step "One more readiness check specific to locking in"
check "mpd is active"   ssh "$HOST" systemctl is-active --quiet mpd
check "vaino is active" ssh "$HOST" systemctl is-active --quiet vaino

step "B read-only in fstab [BOSE003 step 10]"
# sed, not a blind append: idempotent against a re-run, and against this
# already having been done by hand. Matches the exact line
# provision-bose.sh's own fstab step writes -- if that line's format ever
# changes, this pattern needs to change with it; it will silently do nothing
# rather than error, which is why the actual fstab line is always echoed
# below instead of trusting the sed's own exit status.
run "set LIBRARY ro in /etc/fstab" ssh "$HOST" \
    "grep -q 'LABEL=LIBRARY.*[^r][^o][[:space:]]' /etc/fstab && sudo sed -i \
        's#\\(LABEL=LIBRARY[[:space:]]\\+/srv/library[[:space:]]\\+ext4[[:space:]]\\+\\)defaults#\\1ro#' \
        /etc/fstab || true"
say "verify this line actually says 'ro', not just that the command exited 0:"
say "$(ssh "$HOST" grep LIBRARY /etc/fstab)"

caveat \
    "About to enable the overlay and write-protect /boot/firmware. This is" \
    "[IMPL-BOS-120]'s point of no easy return -- after this, a mistake in" \
    "/etc means another card swap. The call below was verified by reading" \
    "raspi-config's source on THIS card on 2026-09-06, not by having run it."
step "Enable overlay on A + write-protect /boot/firmware [do_overlayfs]"
run "raspi-config nonint do_overlayfs 0" ssh "$HOST" sudo raspi-config nonint do_overlayfs 0
say "Read the output above: raspi-config prints what it actually did."

step "Enable vaino and mpd at boot (not done until now, deliberately)"
run "systemctl enable mpd vaino" ssh "$HOST" "sudo systemctl enable mpd vaino"

step "Reboot into the locked-down card"
say "This is the point of no easy return [IMPL-BOS-120]. Rebooting now."
ssh "$HOST" "sudo reboot" || true
say "bose is rebooting. Give it a minute, then verify:"
say "  ssh $HOST 'findmnt -no FSTYPE /; findmnt -no OPTIONS /srv/library; systemctl is-active vaino mpd'"
say "Log: $LOG_FILE"
