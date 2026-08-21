# SPEC017: What Counts as a Play

**Design Specification — Tier 1 · one rule, every path**

`listener_play_history` is the only irreplaceable data in the system
`[SPEC-DF-090]`, and rotation, recovery and restraint are all computed from it
`[SPEC009]`. So the question of *when a row is written* is not an implementation
detail of whichever player happened to be running.

This document exists because it briefly was one. The local engine wrote a play
at the **start** of a passage; the MPD Director wrote one at **half its length**.
Both wrote the same table.

---

## 1. The rule

**`[SPEC-PLAY-010]` A passage counts as played once the listener has heard half
of it, or four minutes, whichever comes first.** Measured against the **passage
span**, not the file's length — two passages in one capture have different
lengths and identical file durations `[SPEC-DF-030]`.

Both Last.fm and ListenBrainz use exactly this threshold. Adopting it rather
than inventing one buys three things: it is already tuned by services that have
watched billions of listens, it agrees with whatever scrobbler the listener
already runs `[SPEC-MPD-100]`, and it is a number a person can check.

**`[SPEC-PLAY-020]` The minimum-length exclusion is deliberately not adopted.**
Last.fm additionally ignores tracks under 30 seconds and ListenBrainz under 5.
That floor is an anti-spam rule about fraudulent submissions to a public
service, and it does not apply to a private rotation ledger. Vaino's shortest
radio passage is **12 seconds** `[SPEC-SA-090]`; one that played in full did
play, and excluding it would suppress nothing but the truth.

**`[SPEC-PLAY-015]` An unknown length is never a play.** Half of zero is
trivially reachable, so a passage that had not played at all passed the
threshold — observed, in the MPD observer's first live run. Absent is not zero
`[GOV-SRC-040]`.

**Measured against what was *heard*.** For the local engine that is audible
position, net of output buffering — the ring holds seconds of audio, and
decoder position would count music the listener never got to.

---

## 2. It applies to every path

**`[SPEC-PLAY-030]` One rule, one implementation, called from both.**
*(Settled 2026-08-21.)* `player/src/scrobble.rs` is the only place the threshold
is expressed; the local engine and the MPD Director both call it. Restating it in
two places is how the two paths drifted apart in the first place, so the rule is
imported rather than repeated — including by the prototype binaries.

| path | reads the rule from | writes |
| :--- | :--- | :--- |
| local engine `[REQ-PD-110]` | `scrobble::counts_as_play` | `listener_play_history` |
| MPD Director `[SPEC-MPD-090]` | the same function | the same table |
| future backends `[GDE-BAK-100]` | the same function | the same table |

---

## 3. The divergence this created, recorded

**`[SPEC-PLAY-040]` This is a measured divergence from MuLibPlay, taken
deliberately.** *(Changed 2026-08-21.)* MuLibPlay's own note says its history
structures update "as each new track finishes playing (or is put in the play
queue)", and Vaino's engine followed it: a play was written the moment a passage
began sounding. The reasoning was explicit — rotation spaces out what the
listener has *encountered*, and a track skipped after ten seconds has been
encountered.

That reasoning is not wrong, but it answers a different question from the one
`listener_play_history` is now asked. `[REQ-PD-110]` requires MuLibPlay's
*weighting* be reproduced as designed; this changes *what feeds* the weighting,
which is the class of change `[SPEC-DIR-210]` already defers under
`[GDE-PHS-030]` — and it is recorded here rather than absorbed silently.

> **Consequence, stated rather than discovered later: a skipped passage is no
> longer suppressed.** Under the old rule, skipping a track after ten seconds
> pushed it down the rotation for a while. Under this one it was never played, so
> nothing suppresses it and the Director may offer it again soon. Whether a
> *skip* should carry its own short suppression, separate from a play, is
> **open** — it is a different mechanism, not a tuning of this one, and it should
> not be smuggled in as a side effect of aligning the threshold.

---

**Traceability:** `[SPEC-PLAY-010..040]` · implemented in `player/src/scrobble.rs`
· consumed by `[REQ-PD-110]`, `[SPEC-MPD-090]` · rationale
[SPEC015](SPEC015-mpd-director.md), [SPEC016](SPEC016-mpd-protocol-findings.md)
