# BOSE006: What Lands Where on the Card

**Appliance Record — the contents of each partition, and why each is there**

Split from [BOSE002](BOSE002-image-build.md) on 2026-09-10, which had reached
327 lines against `[GOV-DOC-010]`'s 300-line limit.

> **Related:** [BOSE002](BOSE002-image-build.md) for the layout ·
> [BOSE003](BOSE003-build-procedure.md) for building it

---

> **Section numbers below are the pre-split document's.** This file was carved out of a larger one on 2026-09-10, and its cross-references still use the original numbering: §1-2 and §4 in [BOSE002](BOSE002-image-build.md), §3 in [BOSE006](BOSE006-what-lands-where.md).

## 3. What lands where

**A — system, read-only.** Kernel, Bookworm Lite, `vaino` binary and unit,
`mpd` and its unit drop-in, `/etc`. `[PI-A-030]`: nothing about the library or
the listener.

**B — library, read-only except during import.** The 44 GB of audio and cover
art, and the `.cue` sheets if they are ever enabled `[REQ-VIS-205]`. MPD's own
database — its index — belongs here too and not on A: it is derived from B's
contents, it is rebuilt by `update`, and it changes exactly when B changes.

**`[IMPL-BOS-078]` `vaino.db` itself goes on C, not B — a correction, found
building this card.** This section originally listed `library.db` as living
on B, per `[PI-DB-010]`'s split design. That split was never built: the
player still opens one `vaino.db`, no `ATTACH`, and the schema stays one file
per `[SPEC-SC-010]`. So there is no separate `library.db` to place — only the
one file, and it has to go somewhere that stays writable, because every play
is a write to it.

`vainopi` gets away with the same single file sitting under `/srv/library`
(`[PI002](../VainoPi/PI002-test-image-setup.md)`,
`[PI005](../VainoPi/PI005-appliance-library.md)`) only because that partition
is never actually made read-only in practice — `vainopi` has no C, and its
data partition just stays `rw` forever. `bose` is not that: step 10 of
`[BOSE003](BOSE003-build-procedure.md)` genuinely flips B to `ro`. Putting
`vaino.db` there would mean every play after that point fails to write. So
until the split is built, `vaino.db` lives at **`/var/vaino/vaino.db`** — C,
not B — and B holds only audio, cover art, and MPD's own derived index.

**`[IMPL-BOS-150]` The attended-import operation, finally built.**
`[PI-B-030]` said what this should be — "remount rw, run the ingest, sync,
remount ro" — from this project's earliest design pass; nothing carried it
out until now. Asked directly, checking the actual scripts rather than the
design doc, found the gap: B isn't even set `ro` by default
(`provision-bose.sh`'s own `fstab` line is plain `defaults,noatime`, `rw`
until `[BOSE003]` step 10's `--lock-in` flips it once), and nothing
performed the reopen-import-reclose cycle, because `seed-library.sh` never
needed to — it always ran before B was ever `ro` in the first place.
`BosePi/attended-import.sh` (dev host, over SSH, the same home every other
phase script has) closes it:

1. Reads B's *current* mount options and remembers them, whatever they are
   — `rw` pre-lock-in, `ro` after. This is what makes "restore afterward"
   correct in both worlds, rather than assuming lock-in has already
   happened.
2. Remounts `rw` only if it was not already — an asked-twice `--check`/
   `--go` decision, the same discipline `[IMPL-BOS-110]`'s destructive
   scripts already use: a wrapped command that misbehaves can corrupt
   exactly the partition this whole design protects, so nothing here
   defaults to acting.
3. Runs the caller's command verbatim inside the window — new audio via
   `rsync`, a mesh-approved bundle's `audio/` directory, whatever is
   needed. The script has no opinion on what belongs in the window, only
   that the window exists and closes.
4. `sync`s, then restores exactly the options step 1 captured — via a
   trap, so a wrapped command that fails or is interrupted still closes
   the window rather than leaving B open indefinitely.
5. Best-effort MPD reindex afterward (skippable with `--no-mpd-update`) —
   B's own index changed if the audio did, and every caller adding audio
   needs this, so it is default-on rather than one more step to remember.

**Run for real against `bose` 2026-09-06, four ways**: a dry run, a
successful command, and a failing one (confirmed B still closes and nothing
after it runs) against a hand-simulated `ro`, before `--lock-in` had ever
run — then a fourth run against `bose`'s own real `--lock-in`'d `ro` once
it existed, confirming the hand-simulated tests generalized correctly.
MPD's reindex trick needed no `mpc` client
(`provision-bose.sh` never installs one) — bash's own `/dev/tcp` speaks the
control port directly.

**`[IMPL-BOS-155]` This is the missing half of a mesh-approved import, not a
separate feature.** `[SPEC035](../docs/spec/SPEC035-mesh-library-sync.md)`'s
`mesh_diff.py`/`resolve_mesh_conflict.py` only ever touch
`/var/vaino/vaino.db` — correct, since C is what stays writable regardless
of lock-in — but a `local_only`/`peer_only` bundle's audio bytes still need
somewhere to land, and B was the half nothing wrote yet.
`attended-import.sh` is that half, run the same way the original seed
(`[IMPL-BOS-085]`) did before B was ever locked: `rsync` the bundle's
`audio/` into `/srv/library/audio/` inside the window it opens.

**`[IMPL-BOS-160]` The lock-in escape hatch — a flag file, checked at boot,
that undoes `--lock-in` without a card pull.** `[IMPL-BOS-120]` calls
`--lock-in` "the point of no easy return" honestly: after it, a mistake in
`/etc` means another card swap, because `raspi-config`'s own internals are
not something to reverse-engineer under pressure. This narrows that to two
reboots, for the case that actually matters most — the system boots fine,
and someone just wants A writable again.

A marker file in either of two places, checked by a systemd unit
(`vaino-unlock-check.service`) ordered right after both mount:

| Marker | Partition | Reachable when |
| :--- | :--- | :--- |
| `/var/vaino/unlock/request` | C (`f2fs`) | SSH still works — no card needed at all |
| `/boot/firmware/unlock-request` | boot (`vfat`) | SSH is broken, any reader can still write plain FAT |

Finding either, `vaino-unlock-check.sh` edits `/boot/firmware/cmdline.txt`
directly, removing the whole `overlayroot=...` token, and reboots. Two
reboots to actually land in a writable system: this one notices the marker
and flips the config; the next one boots into the result. The marker is
deleted only if the token is actually gone afterward — checked directly,
not assumed from a command's exit status — a failure retries on the next
boot instead of silently giving up.

**Must exist on A before `--lock-in` ever runs, not after — the whole
reason this is worth saying plainly.** Once A is the overlay's read-only
lower layer, adding anything to it needs the overlay disabled first, which
is exactly what this script exists to do. `provision-bose.sh` installs it
alongside `vaino`/`mpd`, so every card built from here on has the escape
hatch before it is ever locked.

**Deliberately narrow, restated so it is not oversold.** Rescues "A boots,
and I want it writable" — nothing running under systemd can rescue a boot
that never gets that far, which still needs a card and a reader, the same
as today. It does not touch B's temporary-reopen case at all; that is
`attended-import.sh`'s job, needs no reboot, and was already solved before
this existed.

**`[IMPL-BOS-175]` The paragraph above is wrong as built: the hatch does NOT
leave A writable, and on this machine it strands it off the network. Do not
use it to install software — use `overlayroot-chroot`. Found 2026-09-11 by
using it.** `bose` was unlocked in order to `apt-get install chrony`, and did
not come back: no SSH, no ping, and `ip neigh` on a LAN peer reporting
`FAILED` — nothing at layer 2 claiming the address. Recovery took a card and
a reader, which is precisely the situation this hatch exists to avoid.

The mechanism is two locks and one key. `[BOSE003]` step 10 sets **both** the
overlay on A *and* `ro` in `fstab` for A. The hatch removes only the
`overlayroot=` token from `cmdline.txt`; `fstab`'s `ro` survives it, and the
one service that would otherwise remount `/` read-write was deliberately made
a no-op by `[IMPL-BOS-170]` because that remount always failed against an
overlay root. So A comes up **read-only with no tmpfs upper layer to absorb
writes** — strictly worse than the locked state, where every write at least
succeeded in RAM until the next reboot.

That is fatal here rather than merely awkward, because `bose` is wireless
`[PI-BOS-020]` and NetworkManager needs a writable `/var/lib/NetworkManager`
to associate. No writable `/var`, no Wi-Fi, no way in.

**`[IMPL-BOS-180]` `overlayroot-chroot` is the right tool for any persistent
change to a locked card, and it needs no reboot at all.** `[BOSE003]` already
said so — *"writes the same drop-in through to A's real lower filesystem
without needing the full unlock/reboot/relock cycle"* — but said it inside a
paragraph about swap, where nobody looking for "how do I install a package"
would find it. Stated here plainly instead:

```bash
sudo overlayroot-chroot apt-get install -y <pkg>     # persists on A
```

For something that must also take effect *now*, write twice — through the
chroot for A, and to the live overlay — the same treatment `[IMPL-BOS-170]`'s
own two fixes used on 2026-09-07.

Worked example, the chrony install that `[IMPL-BOS-175]` got wrong, done the
right way once `bose` is back — `[GDE-ECHO-300]` wants every node on one LAN
reference, which is `smartboardpc` at 192.168.67.93:

```bash
ssh pi@bose sudo overlayroot-chroot apt-get install -y chrony
# the fleet source file, written to A and to the live overlay both
printf '%s
' 'server 192.168.67.93 iburst prefer minpoll 4 maxpoll 6'   'pool us.pool.ntp.org iburst maxsources 3' 'server time.nist.gov iburst'   | ssh pi@bose 'sudo overlayroot-chroot tee /etc/chrony/sources.d/vaino-fleet.sources'
ssh pi@bose sudo systemctl restart chrony   # live half; no reboot needed
```

Verify with `chronyc sources` showing `^* smartboardpc.lan`, then confirm it
survived by checking again after the next ordinary reboot — `[IMPL-BOS-165]`'s
standing lesson is that the file, not the exit code, is the evidence.

**Until the hatch sets `fstab` back to `rw` as well, treat it as unusable.**
It cannot edit `fstab` from inside the overlay — that is why the scope was
narrowed in the first place — so the honest repair is to drop the
`systemd-remount-fs` no-op and set A's `fstab` entry `rw`, or to retire the
hatch in favour of `overlayroot-chroot` and keep the card reader as the only
rescue.

**`[IMPL-BOS-166]` The first version called `raspi-config nonint
do_overlayfs 1` instead, and it silently did nothing — found on the first
real test of the actual enabled-to-disabled transition, 2026-09-06.**
`disable_overlayfs()`'s own implementation runs `sed -e
"s/\(.*\)overlayroot=tmpfs \(.*\)/\1\2/"` — a literal match against
`overlayroot=tmpfs ` with nothing between `tmpfs` and the next space.
`[IMPL-BOS-165]`'s own fix, `overlayroot=tmpfs:recurse=0`, means that
literal string never appears again, so `raspi-config`'s sed matches
nothing, changes nothing, and still reports success. One correction's own
fix broke the mechanism the next correction depended on — found only
because the actual file was checked afterward rather than the exit code
trusted, the identical discipline `[IMPL-BOS-165]` had just demonstrated
was necessary, applied a second time the same evening. Rewritten to edit
`cmdline.txt` directly (removing `overlayroot=` plus whatever follows it,
regardless of parameters, so it cannot go stale the same way again) rather
than route through `raspi-config` for this one step. Confirmed against a
real enabled-to-disabled transition, not only the already-disabled no-op
case the first version was tested against — twice, since the first fix
attempt had to be applied by hand (A was still locked, and the broken
escape hatch could not yet unlock itself) before the corrected script
could even be deployed durably.

**Scope was also narrowed in the same pass.** The original version also
tried to restore B's `fstab` line to `rw`. Dropped: any edit to a file
living on A — `/etc/fstab` included — made while A is *still* overlaid
lands in the RAM upper layer and is lost on the very reboot the script
itself triggers, the same transient-write trap this whole design exists to
avoid, self-inflicted. Restoring A alone is real and durable because
`cmdline.txt` lives on the boot partition, which is never overlaid;
reopening B was always `attended-import.sh`'s job and is unaffected.

`BosePi/request-unlock.sh` is the human-facing half: writes the C-side
marker over SSH and reboots, asked twice like every other script here that
undoes something deliberate.

**First run, before `bose` was ever `--lock-in`'d**: marker written, one
reboot, log confirmed `do_overlayfs 1 exit 0` and the marker cleared — but
only the already-unlocked no-op case, since there was nothing yet to
disable. The real enabled-to-disabled transition came later, against a
genuinely `--lock-in`'d `bose`, and is what `[IMPL-BOS-166]` above is
about: that first version turned out not to work at all once there was
something real to undo, and the fix proven afterward is what's actually
running now.

**C — state, read-write, the only continuously written partition.**

| Path | What |
| :--- | :--- |
| `/var/vaino/vaino.db` | the whole database — library cache and listener state together `[IMPL-BOS-078]` |
| `/var/vaino/log/` | `/var/log` bind-mounted here `[PI-A-020]` |
| `/var/vaino/mpd/` | MPD `state_file`, `sticker.sql`, playlists |
| `/var/vaino/backup/` | `[REQ-LIB-160]` snapshots, pending copy off-device `[PI-C-030]` |
| `/var/vaino/etc-ssh`, `/home-pi`, `/nm-connections` | bind-mount sources for host identity and known networks `[PI-A-025]` |

**MPD's four state files split across two partitions**, which is the same
`[PI-DB-010]` line drawn again: `db_file` is derived from the library and lives
on B; `state_file`, `sticker_file` and `playlist_directory` are the listener's
and live on C. `vainopi` puts all four together under `/srv/library/mpd`
because it has no C to put them on; this image should not copy that.

---

