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

---

## 4. A skip suppresses, and does nothing else

Aligning the threshold left a gap: under the old rule a ten-second skip pushed a
track down the rotation, and under `[SPEC-PLAY-010]` it was never played, so
nothing held it back at all. The answer is not to loosen the threshold — a skip
is genuinely not a listen — but to give the skip its own narrow effect.

**`[SPEC-PLAY-050]` A skipped passage is held out of selection for a window, and
that is its entire consequence.** *(Settled 2026-08-21.)* It is written to
`listener_skip_history`, never to `listener_play_history`, and the eligibility
gate is the only thing that reads it. It contributes **no** play count, **no**
recovery ramp, **no** artist mark and **no** weight of any kind — a passage whose
window has passed weighs exactly as one never skipped, which is asserted rather
than described.

Structurally rather than by convention: `skip_age_s` is a separate field from
`track_age_s`, the gate sits with the passage filters *above* the artist and
track passes, and nothing below it reads the value. A skip cannot leak into a
ramp because there is no path from one to the other.

**`[SPEC-PLAY-060]` The window is the listener's, default 156 hours.** Six and a
half days: long enough that a rejected passage does not return within the week,
and deliberately offset from a whole week so it does not come back on the same
day at the same time. Edited on the Vaino skin settings page and persisted like
the other tunables `[REQ-VIS-155]`, taking effect live rather than at the next
rebuild. **Zero is legitimate** and turns suppression off, which is why the
window is a number and not an on/off switch with a number beside it.

> **A skip is deliberately not stored per passage.** The window is keyed by
> recording MBID, like rotation itself, so rejecting one encoding of a song
> suppresses the song rather than sending the selector to a different copy of the
> take the listener just refused.

`listener_skip_history` is **Class D** and never travels `[SPEC-DF-055]`: it is
an account of one listener's reactions, which is exactly the material that must
not ride along in a payload.

---

**Traceability:** `[SPEC-PLAY-010..060]` · implemented in `player/src/scrobble.rs`
and `player/src/director/frequency.rs`
· consumed by `[REQ-PD-110]`, `[SPEC-MPD-090]` · rationale
[SPEC015](SPEC015-mpd-director.md), [SPEC016](SPEC016-mpd-protocol-findings.md)
