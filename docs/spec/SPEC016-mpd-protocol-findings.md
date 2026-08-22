# SPEC016: What MPD Actually Does

**Design Specification — MPD protocol behaviour, measured**

Companion to [SPEC015: MPD Director](SPEC015-mpd-director.md). That document is
the design; this one is the set of places where MPD's observed behaviour differs
from what its protocol documentation implies, each measured against this library
rather than argued from the manual.

Split out of SPEC015 on 2026-08-21 under `[GOV-DOC-010]`. They are separated
because they answer different questions and change for different reasons: the
design changes when Vaino's intent changes, these change when MPD does.

Every clause here was found by running the prototype `[IMPL-MPD-010..030]`, not
by reading. That is the argument for building stages before writing them down.

---

## 1. Duration

**`[SPEC-MPD-092]` The length the threshold is measured against is Vaino's, never
MPD's.** *(Decided 2026-08-21, from measurement.)* MPD reports `duration` from an
estimate rather than a decode — for a VBR MP3 it is size over bitrate, so
embedded cover art inflates it. Across the 5,373 files both libraries know, **36.9%
disagree by more than a second**, median error **98.8 s**, worst `+3421 s`; **1,530
of those move the play threshold**, and the errors run in *both* directions, so no
correction factor rescues it. Judged against MPD's figure, a 12.07 s track that
played *in full* was recorded as a skip.

So the resolution ladder `[SPEC-MPD-060]` is load-bearing for **judging**, not only
for enqueuing: a URI that does not resolve to a Vaino passage yields a verdict
resting on an estimate, and must be reported as the weaker claim it is. Where the
passage span is known it supersedes even the file duration, being authoritative by
construction `[SPEC-DF-030]`.

> `ffprobe` was asked which side was right. Vaino matched it to the millisecond
> in every case checked, which is what makes this a ranking `[GOV-SRC-020]` and
> not a preference.

---

## 2. Ranges

**`[SPEC-MPD-096]` `rangeid` returning `OK` is not evidence the span landed.**
*(Decided 2026-08-21, from measurement.)* MPD validates the requested end against
**its own duration estimate** — the unreliable one `[SPEC-MPD-092]`. Where the end
exceeds it, MPD accepts the command, silently **drops the end**, reports a
shortened `Time`, and then plays **to end of file** regardless. Measured: 508 of
7,994 resolvable passages (**6.4%**), median overrun 11.2 s past the intended end,
worst 532 s. None fell on a multi-passage capture, so nothing spills into a
neighbouring song — but that is this library's luck, not a property of the
protocol.

So every `rangeid` is **read back** and the resulting `Time` compared against the
span asked for; a mismatch is reported rather than assumed away `[GOV-SRC-030]`.
Withdrawing the passage instead would make 6.4% of the library unplayable through
MPD, which is worse than a known overrun. The Director enforces the end itself,
by advancing at the span boundary using the sampler it already runs — bounding the
overrun by the sample interval `[SPEC-MPD-105]` rather than by the file's length.

> The residual imprecision is real and is the cost of being a guest: the end
> lands within one sample interval rather than exactly. Vaino's own engine has no
> such limit, which is a fair statement of what the MPD path gives up.

**Where `rangeid` does hold, it holds exactly.** With an end inside MPD's estimate,
400 queued passages matched their spans to within **1.0 ms**, 127 of them mid-file;
`elapsed` runs relative to the range start and `duration` reports the span, so a
passage inside a multi-hour capture needs no coordinate translation at all.

---

## 3. Naming

**`[SPEC-MPD-052]` MPD names a passage from the file's tags, and a capture has
none.** *(Measured 2026-08-22, reported by a listener.)* A DAO capture is one
file carrying one set of album-level tags — no per-track title at all — so a
client reading tags shows the album, or nothing, for every passage inside it.

**2,840 of 8,330 radio passages (34.1%) live in such a file.** A third of
everything the Director can choose arrives at a client unnamed.

**And the obvious remedy is refused.** `addtagid` exists, is advertised in
`commands`, and overrides a queue entry's tags — but only for a song MPD does
not have in its database:

```
addtagid {id} Title "…"  ->  ACK [4@0] {addtagid} Cannot edit tags of local file
```

So per-passage naming genuinely cannot be expressed through the queue, which is
why `Capabilities::MPD` declares `naming: false` rather than treating this as a
defect to be worked around. Vaino publishes `vaino.title` and `vaino.artist` as
stickers instead `[SPEC-MPD-050]`: available to any client that reads them, and
honestly absent from those that do not.

> The capability model missed this until a listener saw album names where track
> names belonged. `spans`, `gain` and `ramps` were all about *playing* a
> passage, and nobody asked whether the guest could **say what it was**.

**`[SPEC-MPD-054]` And the sticker is not a substitute, measured against a real
client.** Cantata shows `TestForEcho` and `GoldAsia` — MPD's filename fallback —
for captures whose `vaino.title` sticker was set correctly at the same moment.
Stickers make the truth *available*; they do not make a client show it, and no
client is obliged to look. Publishing them is still right, and it is not a fix.

Two further routes were tried and closed:

| route | result |
| :--- | :--- |
| `addtagid` on a database song | `ACK [4@0] Cannot edit tags of local file` |
| `addid "file:///abs/path"`, to get a song MPD has no database entry for | `ACK [4@0] Access to local files not implemented on **Windows**` |

The second is a *platform* limit rather than a protocol one, so it may work on
the appliance's Linux MPD and is **untested there**. If it does, a passage could
be added by absolute path and then named with `addtagid` — correct titles in
every client, at the cost of losing the database entry the sticker hangs on.

**`[SPEC-MPD-056]` The route that would actually fix it is a cue sheet.** This
build carries the `cue` and `embcue` playlist plugins, and MPD exposes a cue
sheet's tracks as separate songs with their own titles and offsets. A capture
with a cue sheet beside it would name every passage inside it, for every client,
with no sticker support required.

**Taken, behind `[REQ-VIS-205]`.** A cue track is added by URI and **`rangeid` is
not called on it**: MPD applies the cue's own boundaries as a range and reports
the real title, and a `rangeid` would overwrite that with offsets into the file
and lose both. Measured on the queue: cue start 1324.7 s against a radio start
of 1324.7 s — exact — and a cue end 3.8 s late, which the sampler ends itself
`[SPEC-MPD-096]` rather than accepting.

What follows is why the *default* is off, not why it was declined:
it means writing files into the listener's music tree, and Vaino's passages are
not always a CD's tracks — a radio span trims what an album span keeps
`[GDE-BMK-030]`. That is a decision about what Vaino is allowed to do to a music
folder, and it belongs to the person who owns the folder.

---

## 3. Transport state

**`[SPEC-MPD-094]` A stop ends a passage; a pause does not.** MPD retains `songid`
across a stop, so a watcher keyed on the song identity alone never notices one — a
track stopped past its threshold went unrecorded, and stayed unrecorded if nothing
played after it. A pause is the opposite case: elapsed holds still, the listener
is coming back, and closing the book on them would count a play as a skip.

**`[SPEC-MPD-099]` MPD cannot be asked to fade, and may not be able to at all.**
*(Measured 2026-08-21.)* The protocol has a global `crossfade` between songs and
it has `stop`. There is no "fade out over N ms", so a fade must be built from
`setvol` steps — and `setvol` is refused outright with **`ACK [5@0] {setvol} No
mixer`** when the output plugin has none. MPD's `null` output has none; a
PipeWire or ALSA output, which is what an appliance runs, does.

Which makes it testable on a development machine after all, and that is worth
writing down because the first attempt concluded otherwise: MPD's Windows build
carries `wasapi`, and

```
audio_output { type "wasapi"  name "…"  mixer_type "software" }
```

reports a `volume` and accepts `setvol`. The fade path was thought to need the
appliance because the *test rig* had a `null` output, not because Windows could
not do it.

So a fade *out of* MPD is conditional on the deployment rather than on the
protocol. `[SPEC-BK-030]`'s crossfade is therefore asymmetric: Vaino can always
fade its own side, and MPD can only sometimes fade its own. Which happens is
reported rather than assumed `[PI3-API-030]`.

**`[SPEC-MPD-098]` Clearing a queue while playing stops MPD.** *(Observed
2026-08-21.)* Not a Vaino decision but load-bearing for one: it is what makes
`[SPEC-MPD-120]`'s activation rule close on itself, since the gesture that means
"leave my queue alone" and the gesture that means "stop" turn out to be the same
gesture. Relatedly, `play` on an **empty** queue returns `OK` and leaves the state
`stop` — there is no error to detect, which is why the Director cannot start a
session from cold.

---

**Traceability:** `[SPEC-MPD-092]`, `[SPEC-MPD-094]`, `[SPEC-MPD-096]`,
`[SPEC-MPD-098]` · measured by `[IMPL-MPD-010..030]` · design in
[SPEC015](SPEC015-mpd-director.md) · ranking discipline `[GOV-SRC-010]`
