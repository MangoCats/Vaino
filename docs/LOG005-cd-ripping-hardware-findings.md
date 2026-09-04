# LOG005: CD Ripping — Hardware Findings

**Development Record — Tier 0**

Dated incident record behind [SPEC025](spec/SPEC025-cd-ripping.md) §5a's
failure-handling design. SPEC025 itself describes only current, decided
behaviour; this is where the first real-hardware testing against it, and
what it found, is kept, per `[GOV-DOC-050]`.

---

## 2026-09-03 — A USB-attached drive's DAE returns noise, not audio

**`[LOG-RIP-010]` An investigative DAO capture (`cdrdao`, Weezer "(White
Album)") converted to `.ogg` played back as static.** The TOC/CD-TEXT read
was genuine and correct — real track titles, ISRCs, matching the physical
disc — and the `.bin`'s byte count matched the TOC's own total duration
exactly, so the capture was not truncated. But the audio payload failed
the one test that actually distinguishes real audio from noise: lossless
FLAC compression. Two independently-sampled 10MB chunks (start of track
1, mid-album track 5) both *expanded* under FLAC (+0.6%, +0.8%) instead
of compressing — real audio, even the loudest modern master, always has
enough inter-sample/stereo correlation for FLAC to shrink it; only data
statistically close to random noise turns FLAC's frame overhead into a
net loss. Spectrograms confirmed it visually: flat, uniform broadband
energy, no harmonic structure, at both sampled points.

**`[LOG-RIP-020]` The same signature reproduced on a second, unrelated
disc, isolating the fault to the drive rather than that one disc.** Two
bounded test extractions (`timeout N cdrdao read-cd`, 3s and ~90s
wall-clock) against whatever disc was in the drive at the time — a
different, 14-track CD-DA disc — produced the identical result:
`cdrdao disk-info` read its TOC correctly, but the ~130-second-equivalent
raw-audio sample again expanded under FLAC (+1.4%) with the same flat
spectrogram. Two different discs, same drive, same failure — this rules
out "one bad disc" and points at the extraction path itself.

**`[LOG-RIP-030]` Diagnosis: this drive's DAE fails via `cdrdao`'s
generic-SCSI driver on this Linux machine; TOC-level reads do not.** The
drive is an HL-DT-ST DVDRAM GP65NS60, USB-attached (`lsblk` reports
`TRAN=usb`). `cdrdao disk-info` and the disc's own CD-TEXT/Q-subchannel
resolve correctly — the low-bandwidth control commands work. The bulk
raw-audio transfer (`READ CD`) `cdrdao`'s Generic SCSI-3/MMC driver
issues over this connection does not — a known failure mode for USB
optical bridge chipsets that pass ordinary block reads through cleanly
but don't correctly implement the raw-sector command DAE needs,
returning filler data rather than a clean SCSI error. This is consistent
with, and gives a first real-world data point toward, the reasoning
`[SPEC-RIP-020]` already gives for defaulting to EAC on Windows rather
than `cdrdao` there — though it does not yet confirm it, since the same
physical drive has not yet been tried via SPTI.

**Next.** Continuing on a Windows machine with EAC, same physical USB
drive. The next data point is whether EAC/SPTI succeeds where
`cdrdao`/generic-SCSI failed — supporting `[SPEC-RIP-020]`'s reasoning —
or the drive itself cannot do DAE regardless of host or driver, a
different and harder finding `[SPEC-RIP-020]` does not anticipate.

---

## 2026-09-03 — Shopping research: candidate replacement drives, in parallel

**`[LOG-RIP-040]` The failing drive's own class — a slim, all-in-one USB
combo unit — is the likely suspect, not its brand.** HL-DT-ST/LG's own
BD-RE BH14NS48 tops a crowdsourced 2024 ripping-accuracy survey (99.73%
across 123 submitters) on the dBpoweramp forum's long-running
["CD Drive Accuracy"](https://forum.dbpoweramp.com/forum/dbpoweramp/cd-ripper/324732-cd-drive-accuracy-2024)
thread, so "avoid LG" would be the wrong lesson from `[LOG-RIP-030]`;
"this specific unit's USB bridge is unverified" is closer to it.

**`[LOG-RIP-050]` Researched while EAC testing proceeds in parallel —
provisional, not yet a decision.** The Latitude 5590 has no internal
optical bay (confirmed against Dell's own
[owner's manual](https://www.dell.com/support/manuals/en-us/latitude-15-5590-laptop/latitude_5590_om/port-and-connector-specifications)):
three USB 3.1 Gen1 Type-A ports plus one USB-C carrying both USB data and
DisplayPort alt-mode, so any replacement is necessarily USB-attached.
Two candidate paths:

- **Path A — single all-in-one unit: Pioneer BDR-XD08 (or XD07).** A
  slim USB 3.2 Gen1/Type-C Blu-ray/DVD/CD writer with a reputation
  specific to the ripping community itself, not just general optical
  drive reviews — named directly in a Linux Mint forum thread asking this
  exact question
  (["which external DVD burner to quality rip CDs?"](https://forums.linuxmint.com/viewtopic.php?t=348859)),
  and Pioneer's BDR-S-series siblings place at the top of the same
  dBpoweramp survey above. Currently purchasable, roughly $120-200
  (Walmart, eBay, Adorama; Amazon's listing is intermittently in stock).
  Lowest effort — one product, no pairing decision — but still an
  all-in-one unit whose specific USB bridge chip isn't published, the
  same category of unknown that likely explains the current drive's
  failure (`[LOG-RIP-030]`).
- **Path B — bare drive + separately-chosen enclosure.** Decouples the
  two variables Path A bundles into one product. LiteOn's iHAS124 has a
  specific, repeated reputation in the same community for very high
  AccurateRip-verified accuracy (reports of 800+ discs at 100%
  confidence, dBpoweramp and Hydrogenaudio forum threads), sold bare as
  an internal slim SATA drive — paired with a **Vantec NexStar** USB 3.0
  slim-SATA optical enclosure (`NST-510S3-BK` for 9.5mm laptop-style
  drives, `NST-520S3-BK` for 12.7mm desktop-style), driverless and
  OS-independent by design. The same community recommends this exact
  pairing for exactly this reason: the error-reporting behaviour of a
  known-accurate internal drive, with the portability a USB enclosure
  adds. More shopping effort (two parts, matching connector/height), but
  each half is independently vetted rather than trusting one vendor's
  unpublished bridge choice — the opposite bet from Path A and from the
  drive that failed here.

**Next.** Provisional pending the Windows/EAC result (`[LOG-RIP-030]`'s
own "Next"): if EAC succeeds on the *same* physical drive, the fault is
in `cdrdao`'s generic-SCSI path on Linux, not the hardware, and neither
path above may be needed at all. Shopping stays a parallel track, not a
committed purchase, until that result is in.

---

**Traceability:** first real-hardware validation of `[SPEC-RIP-052..056]`
(SPEC025 §5a's failure-handling design); informs, does not yet resolve,
`[SPEC-RIP-020]`'s tool-choice reasoning. `[LOG-RIP-040..050]` is shopping
research, not a design decision — nothing here changes SPEC025/SPEC026.
