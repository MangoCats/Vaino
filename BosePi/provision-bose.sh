#!/bin/bash
# SPDX-License-Identifier: MIT
#
# Phase 3 of [BOSE002]: turn a freshly booted Bookworm into the appliance.
#
# **Runs on the development host, over SSH** [IMPL-BOS-100], against a `bose`
# already booted from the card `prepare-card.sh` made. This is the phase that
# can be driven from here, and it is deliberately the largest one.
#
#     BosePi/provision-bose.sh [host]
#
# Idempotent by construction: every step checks before it acts, so a run that
# fails halfway is fixed by running it again rather than by unpicking it.
#
# **It stops before making anything read-only.** Step 10 of [BOSE002 §6] --
# `fstab` ro and the overlay -- is not here, because after it a mistake costs a
# card swap and the machine should have been listened to first [IMPL-BOS-120].
set -uo pipefail

HOST="${1:-pi@bose}"
BIN=player/target/aarch64-unknown-linux-gnu/release/vaino
LIB_MOUNT=/srv/library
STATE_MOUNT=/var/vaino

say()  { printf '  %s\n' "$*"; }
step() { printf '\n== %s\n' "$*"; }
die()  { printf 'provision: %s\n' "$*" >&2; exit 1; }
on()   { ssh "$HOST" "$@"; }

step "Reaching $HOST"
ssh -o ConnectTimeout=10 "$HOST" true 2>/dev/null || die "$HOST is not reachable"
say "$(on 'cat /proc/device-tree/model 2>/dev/null | tr -d "\0"; echo')"
say "$(on 'uname -m; . /etc/os-release; echo $PRETTY_NAME' | tr '\n' ' ')"

# The whole point of [IMPL-BOS-010] was to stop needing a second toolchain.
case "$(on 'uname -m')" in
    aarch64) ;;
    *) die "$HOST is not aarch64 -- it did not boot the new 64-bit card" ;;
esac

step "Partitions"
# Named by label, not by device: the reader may enumerate differently and a
# provisioning script that writes to the wrong partition is [IMPL-BOS-110]
# again, one phase later.
for label in SYSTEM STATE LIBRARY; do
    dev=$(on "blkid -L $label 2>/dev/null")
    [ -n "$dev" ] || die "no partition labelled $label -- was prepare-card.sh run?"
    say "$(printf '%-8s %s' "$label" "$dev")"
done

step "Mounts"
on "sudo mkdir -p $LIB_MOUNT $STATE_MOUNT
    grep -q 'LABEL=LIBRARY' /etc/fstab || echo 'LABEL=LIBRARY $LIB_MOUNT ext4 defaults,noatime 0 2' | sudo tee -a /etc/fstab >/dev/null
    grep -q 'LABEL=STATE'   /etc/fstab || echo 'LABEL=STATE   $STATE_MOUNT f2fs defaults,noatime 0 2' | sudo tee -a /etc/fstab >/dev/null
    sudo mount -a" || die "mount failed"
say "$(on "findmnt -no TARGET,FSTYPE,OPTIONS $LIB_MOUNT $STATE_MOUNT" | sed 's/^/  /')"

step "Packages"
on "sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
        mpd f2fs-tools cloud-guest-utils alsa-utils >/dev/null" \
    || die "package install failed"
say "mpd $(on 'mpd --version 2>/dev/null | head -1')"

step "The DAC"
# [PI-BOS-020]: the overlay lines are the ones the old install already proved
# on this hardware. Verify the card appeared rather than trusting config.txt.
CARD=$(on "cat /proc/asound/cards 2>/dev/null | grep -i hifiberry | head -1")
[ -n "$CARD" ] || die "no HiFiBerry card -- check dtoverlay=hifiberry-dacplus in config.txt"
say "$CARD"
MIXER=$(on "amixer -c 1 sget Digital 2>/dev/null | grep -o '\[[0-9]*%\]' | head -1")
if [ -n "$MIXER" ]; then
    # Reported, not acted on. Reaching this mixer means opening the card
    # directly, which forecloses the handoff crossfade -- see [IMPL-BOS-030]
    # and the note in BosePi/mpd.conf. The shipped config takes the shared sink.
    say "hardware mixer available at $MIXER (unused; see [IMPL-BOS-030])"
else
    say "no 'Digital' control found -- the shipped config does not need one"
fi

step "State layout  [BOSE002 §3]"
on "sudo mkdir -p $STATE_MOUNT/log $STATE_MOUNT/mpd/playlists $STATE_MOUNT/backup
    sudo chown -R pi:pi $STATE_MOUNT
    # /var/log becomes a bind mount onto C, so the overlay never holds it
    # [IMPL-BOS-060]. This is the single largest consumer left otherwise.
    grep -q '$STATE_MOUNT/log' /etc/fstab \
      || echo '$STATE_MOUNT/log /var/log none bind 0 0' | sudo tee -a /etc/fstab >/dev/null"
say "listener.db, logs, mpd state and backups all under $STATE_MOUNT"

step "Journal  [supersedes PI-A-020]"
# Persistent on C, capped -- not Storage=volatile, because volatile is RAM and
# RAM is the resource this whole layout is defending.
on "sudo mkdir -p /etc/systemd/journald.conf.d
    printf '[Journal]\nStorage=persistent\nSystemMaxUse=64M\nRuntimeMaxUse=8M\n' \
      | sudo tee /etc/systemd/journald.conf.d/vaino.conf >/dev/null"
say "Storage=persistent, SystemMaxUse=64M, on C via /var/log"

step "MPD"
[ -f BosePi/mpd.conf ] || die "BosePi/mpd.conf missing"
scp -q BosePi/mpd.conf "$HOST:/tmp/mpd.conf" || die "upload failed"
on "sudo cp /tmp/mpd.conf /etc/mpd.conf
    sudo mkdir -p /etc/systemd/system/mpd.service.d
    sudo systemctl mask mpd.socket >/dev/null 2>&1
    sudo systemctl daemon-reload"
say "installed; db_file on B, state on C  [BOSE002 §3]"

step "Vaino"
[ -f "$BIN" ] || die "no binary at $BIN -- cross-compile first (see build/README.md)"
case "$(file -b "$BIN" 2>/dev/null)" in
    *aarch64*) ;;
    *) die "$BIN is not an aarch64 binary" ;;
esac
scp -q "$BIN" "$HOST:/tmp/vaino.new" || die "upload failed"
on "sudo install -m755 /tmp/vaino.new /usr/local/bin/vaino"
say "$(on '/usr/local/bin/vaino --version 2>/dev/null')"

step "What is NOT done here"
say "- the library copy: BOSE002 §7, from the old card in the USB reader"
say "- the resume interval: [IMPL-BOS-070], set it once listener.db exists"
say "- read-only fstab and the overlay: [IMPL-BOS-120], deliberately last,"
say "  and only after somebody has heard this machine play."

printf '\nProvisioned. Play something before closing the door.\n'
