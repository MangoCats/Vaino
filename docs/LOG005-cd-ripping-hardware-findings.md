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

**Traceability:** first real-hardware validation of `[SPEC-RIP-052..056]`
(SPEC025 §5a's failure-handling design); informs, does not yet resolve,
`[SPEC-RIP-020]`'s tool-choice reasoning.
