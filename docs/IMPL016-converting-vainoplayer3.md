# IMPL016: Converting `vainoplayer3` — Overlay Root, Split Database, Current Build

**Implementation Plan — written 2026-09-15, not yet executed**

`vainoplayer3` is overdue an update and is the least hardened node in the
fleet. This brings it to `bose`'s shape without losing the one thing it has
that `bose` does not: a working touchscreen.

> **Related:** [IMPL013](IMPL013-executing-the-rename.md) `[IMPL-NAM-115]` — owns the rename, and is blocked on this machine · [LOG010](LOG010-power-loss-risk-on-the-pi-nodes.md) `[SD-RISK-010]` — why the root shape is the risk · [BOSE009](../BosePi/BOSE009-image-update-runbook.md) — the runbook this follows · [BOSE010](../BosePi/BOSE010-changing-a-locked-card.md) `[IMPL-BOS-180]` — how to change anything afterwards

---

## 1. It is not a reimage

**`[IMPL-VP3-010]` The partition layouts are already identical, so nothing has
to be repartitioned, reformatted or copied.** Measured 2026-09-15:

| | p1 | p2 | p3 | p4 |
| :--- | :--- | :--- | :--- | :--- |
| `bose` | 512M vfat `/boot/firmware` | 8G ext4 `/media/root-ro` | 4G f2fs `/var/vaino` | 106.6G ext4 `/srv/library` |
| `vainoplayer3` | 512M vfat `/boot/firmware` | 8G ext4 **`/`** | 4G f2fs `/var/vaino` | 106.6G ext4 `/srv/library` |

Same sizes, same filesystems, same roles. The sole difference is that `bose`
mounts p2 read-only beneath an overlay and `vainoplayer3` mounts it read-write.
**The conversion is a package and a kernel parameter**, not an image build —
`BosePi/build-bose-card.sh` and its companions are not needed here.

It also already has two of the three things `[SD-RISK-150]` recommends for
`vainopi`: state on its own f2fs partition, and **zram** rather than an
SD-backed swapfile. Its root carries 2.9 GB of lifetime writes against
`vainopi`'s 89 GB.

## 2. What must survive

**`[IMPL-VP3-020]` The touchscreen is the whole reason this node is different,
and every part of it is outside the player binary.** Inventoried on the live
machine, because none of it is in the repository:

| | |
| :--- | :--- |
| `fbui.service` | `ExecStart=/usr/local/bin/fbui ws://127.0.0.1:5720/ws`, `User=pi`, `Nice=-5`, `Restart=always`, and an `ExecStartPre` that turns the console cursor off on tty1 |
| the binary | built with the **`fbui` cargo feature**, which `bose` and `vainopi` never compile `[SPEC-FBUI-015]` |
| display overlays | `dtparam=spi=on`, `dtoverlay=nospi10`, `dtoverlay=piscreen2r,rotate=90,speed=16000000,fps=20`, `dtoverlay=dwc2,dr_mode=host` |
| a deliberate absence | `dtoverlay=vc4-kms-v3d` is commented out — *"HDMI never used on this appliance, disabled for boot speed"* |
| the port | **5720**, not 80. `fbui` connects to it by URL, so the two move together or not at all |
| the library | `/srv/library` on p4, 44 GB in use |

**The overlay makes every one of these read-only.** After the conversion,
changing a `dtoverlay` line or the `fbui` unit needs `overlayroot-chroot`
`[IMPL-BOS-180]` or `build/install-config.sh`. That is the cost of the
hardening and the reason section 4 orders the work as it does.

## 3. What changes

**`[IMPL-VP3-030]` Four changes, of which only one is risky.**

1. **The build.** It currently runs `a76715eb685d+dirty` — a *dirty* build, so
   nobody can say what code is on it. Replace with a named commit built
   `--features fbui` and without `sampo-support`, per `[GDE-DEP-025]`.
2. **The database.** Still the pre-split monolith at `/var/vaino/vaino.db`,
   where `bose` and `vainopi` are both split. Follow
   [IMPL009](../VainoPi/IMPL009-database-split-plan.md) and `[BOS-RUN-080]`.
3. **The root.** Install `overlayroot`, add `overlayroot=tmpfs` to
   `cmdline.txt`, correct `fstab`, reboot. This is the risky one.
4. **The hostname**, `vainoplayer3` → `lempiplay3`. Owned by
   [IMPL013](IMPL013-executing-the-rename.md), not by this plan.

## 4. The order, and the single constraint that fixes it

**`[IMPL-VP3-040]` Everything that writes to the root must happen before the
root stops being writable.** That one rule determines the whole sequence, and
getting it wrong does not fail — it succeeds, and then vanishes at the next
reboot `[IMPL-BOS-185]`.

| # | Step | Gate before moving on |
| ---: | :--- | :--- |
| 1 | Deploy the current build, `--features fbui`, under the **old** hostname | Plays, **and the screen draws**, and `--version` reports a clean commit |
| 2 | Split the database | Player runs from `library.db` + `listener.db`; old monolith kept, not deleted |
| 3 | Rename to `lempiplay3` | Reachable by the new name; `fbui` still connects on 5720 |
| 4 | Convert the root to overlay | Survives a reboot with the screen working |

Steps 1–3 all write to the root and are ordinary work while it is writable.
Step 4 is last because after it they are not ordinary at all.

Step 1 is also worth doing **immediately and on its own**: it is what
`[IMPL-NAM-115]` has been waiting for, and it is reversible.

## 4a. Step 3 done early, deliberately and alone, 2026-09-15

**`[IMPL-VP3-045]` The hostname was changed on its own, ahead of steps 1 and
2, and nothing else from [IMPL013](IMPL013-executing-the-rename.md) went with
it.** Directed rather than discovered: the larger rename waits for the echo
work to reach a stopping point, and this one change was wanted now.

| | |
| :--- | :--- |
| changed | `/etc/hostname`, and the `127.0.1.1` line of `/etc/hosts` |
| backups | `/etc/hostname.pre-rename`, `/etc/hosts.pre-rename`, on the device |
| **not** changed | `/var/vaino`, the database filenames, the folder layouts, the unit names, and every line of prose — all still `[IMPL013]`'s |
| router name | `vp3-wifi` → `lp3-wifi` at the next re-lease; until then use **192.168.67.27** |
| verified | `hostname` and `hostnamectl --static` agree; `vaino` and `fbui` both active; PCM `RUNNING` with `hw_ptr` advancing at ~44.1 kHz |
| **not** verified | **the screen** — `[IMPL-VP3-090]` says only a person present can confirm that, and nobody has |

**Nothing in the repository broke, because nothing executable referred to the
old name.** All eleven occurrences are prose, source comments, or
`[IMPL013]`'s own audit; no script in `build/` or `tools/` reaches that host by
name. The rename was genuinely isolated, which is why it could be taken out of
order safely.

Taking it out of order costs nothing here: `[IMPL-VP3-040]`'s constraint is
that root writes precede the overlay, and the root is still writable. Steps 1
and 2 remain ahead of step 4 exactly as before.

**For whoever runs [IMPL013](IMPL013-executing-the-rename.md): this host is
already renamed.** Its hostname needs no pass, its `/var/vaino` still does, and
`[IMPL-NAM-115]`'s blocker — that the machine was unplugged — has cleared.

## 5. The traps, all of them already paid for once

**`[IMPL-VP3-050]` Record `cmdline.txt` verbatim before touching it.**
`[IMPL-BOS-175]` took `bose` off the network for hours: the lock-in escape
hatch clears `overlayroot=` from the command line, but `fstab`'s `ro` survives
it, so the machine booted read-only, NetworkManager could not write
`/var/lib/NetworkManager`, and there was no Wi-Fi to fix it with. Recovery
needed the card in a reader and a copy of the original file. Take that copy
first.

**`[IMPL-VP3-060]` `bose` mounts `/boot/firmware` read-only too**, marked
`# overlayroot:fs-unsupported` in its `fstab`. Match that deliberately or
deliberately do not, but do not arrive at it by accident.

**`[IMPL-VP3-070]` Verify the durable copy, never the running one**
`[GDE-DEP-070]`. After step 4, `build/install-player.sh` writes both layers and
checksums the lower one; anything else needs `build/install-config.sh`. Five
days of `bose` deploys were lost to this.

**`[IMPL-VP3-080]` Ask whether `fbui` writes anything.** It draws to the
framebuffer and reads a WebSocket, which needs no writable root — but this has
not been checked, and a cache or state file would silently stop persisting the
moment the overlay goes on. Check before step 4, not after.

## 6. Verifying it, in the terms this project has learned to use

**`[IMPL-VP3-090]` "No errors" is not "working" — on this node less than
anywhere.** `[GDE-ECHO-547]` cost `bose` seven silent minutes while every
counter read healthy, because a stream that never opened cannot underrun. This
node has a second way to be silently wrong: the screen.

Each gate in section 4 wants three separate answers:

- the PCM is **open and its `hw_ptr` advancing**, not merely free of underruns
- the **screen is drawing** — which only a person present can confirm, and is
  the one thing no remote check substitutes for
- after step 4, it all still holds **across a reboot**

## 7. Open

**`[IMPL-VP3-100]` The risk figures for this node have not been computed.**
[LOG010](LOG010-power-loss-risk-on-the-pi-nodes.md) covers `bose` and
`vainopi`; this node sits between them and closer to `bose` than the root
filesystem alone suggests, because its state and library are already on their
own partitions. Worth computing before step 4 rather than after, so the
conversion is justified by a number.

**`[IMPL-VP3-110]` Whether `/var/vaino` becomes `/var/lempi` here is
[IMPL013](IMPL013-executing-the-rename.md)'s call**, not this plan's. It
affects `var-vaino.mount`, `fstab` and the player's argument, and doing it in
the same window as step 3 would be cheaper than twice.
