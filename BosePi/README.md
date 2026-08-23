# BosePi — the second appliance

Everything specific to `bose`: the survey of the machine as found, the image
design, the build procedure, and the scripts and configuration that carry it
out.

**Separate from [VainoPi/](../VainoPi/) because the hardware differs enough to
matter.** `vainopi` reaches a Bluetooth speaker through PipeWire on 464 MB;
`bose` is a Pi 4 with 2 GB and a HiFiBerry DAC+ Pro on I²S. The player is the
same and the partition design is shared `[PI-PART-020]`, but almost every
setting below it lands differently — the output plugin, the mixer, the
architecture of the binary. Interleaving the two sets of notes would have meant
every reader working out which machine each paragraph was about.

## Read in this order

| | |
| :--- | :--- |
| [BOSE001](BOSE001-survey.md) | What is on `bose` today — measured, read-only |
| [BOSE002](BOSE002-image-build.md) | The image design: what goes on which partition |
| [BOSE003](BOSE003-build-procedure.md) | Where each phase runs, and in what order |

## What is here to run

| | |
| :--- | :--- |
| [`prepare-card.sh`](prepare-card.sh) | Phases 1–2. Runs **on `bose`**, against a card in a USB reader |
| [`provision-bose.sh`](provision-bose.sh) | Phase 3. Runs **on the development host**, over SSH |
| [`mpd.conf`](mpd.conf) | The guest's configuration, with its paths split across two partitions |

## State of this work

**Nothing has been executed.** The surveys are measurements; everything else is
a plan. `prepare-card.sh`'s refusal paths have been tested on `bose` — it
declines `/dev/mmcblk0`, a partition, a non-existent device, and a missing
argument — but its destructive path has not been run, because
`[IMPL-BOS-130]`'s card reader does not exist yet.

Two decisions are deliberately left open rather than guessed:

- **`[IMPL-BOS-090]`** — whether MPD should share a sink with the player
  (software mixer, crossfade works) or take the DAC directly (hardware mixer,
  no crossfade). The shipped configuration takes the first, because it is the
  one with evidence behind it.
- Whether the seek workaround `[SPEC-MPD-135]` is needed at all here. It was
  measured against PipeWire feeding Bluetooth `[PI-CHR-100]`; a local card may
  not have the fault.

> **`bose` is somebody's working music player**, with MuLibPlay running on it
> now `[PI-BOS-040]`. Everything here builds a *new* card. The old one stays
> intact and is the way back.
