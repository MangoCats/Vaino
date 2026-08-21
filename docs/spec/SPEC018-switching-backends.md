# SPEC018: Switching Backends Without Stopping

**Design Specification — Tier 2 · measured on the appliance, 2026-08-21**

How a listening session moves between Vaino's own engine and MPD, in either
direction, without a restart and without a reboot.

> **Related:** [SPEC015](SPEC015-mpd-director.md) is the MPD design · [SPEC016](SPEC016-mpd-protocol-findings.md) is what MPD does · [IMPL005](../IMPL005-mpd-prototype-results.md) is what the prototype measured

---

## 1. What the measurements settled

Taken on `vainopi` — Pi Zero 2W, PipeWire and wireplumber, one Bluetooth
speaker — while it was playing, without stopping anything.

| question | answer | why it decides something |
| :--- | :--- | :--- |
| Can two players hold the sink? | **Yes.** Two clients on MIDDLETON, both `[active]` | handoff is possible at all |
| How fast does a client attach? | **85–114 ms** over three trials | the sink is not the bottleneck |
| Cold Vaino to first audio? | **15 s** (≈10 s of it the Director) | starting a player *is* the bottleneck |
| Memory, and headroom? | 464 MB total, **Vaino 100 MB RSS**, 229 MB free, **no swap** | two Directors will not fit comfortably |
| MPD queue Vaino can name? | **5,518 of 5,709** files | 191 captures cannot be adopted |

**`[SPEC-BK-010]` The first row corrects a published claim.** IMPL004 stated
that one Bluetooth sink cannot be held by two players and that MPD therefore
required stopping Vaino. It was never tested and it is wrong: PipeWire mixes.
Everything below follows from that being false.

---

## 2. The shape: one process, two backends

**`[SPEC-BK-020]` The Director loads once, and the *backend* changes underneath
it.** Not two processes handing a session to each other — one process choosing
where its queue is played.

The measurements force this. A second resident player costs another **100 MB**
against **229 MB free with no swap**, and a *non*-resident one costs **15
seconds** to load before it can make a sound. Neither is acceptable for
something a listener does on impulse. Loading the Director once and switching
the output costs neither.

The seam already exists. `Playback` and `Capabilities` were written during the
stage-1 spike `[GDE-BAK-040]`, and `Engine` satisfies the trait unchanged:

```
Capabilities::FULL  { spans: true, gain: true,  ramps: true  }   Vaino's engine
Capabilities::MPD   { spans: true, gain: false, ramps: false }   MPD as a guest
```

**`[SPEC-BK-025]` Switching backend is not switching player.** The Program
Director, the library, the flavor index, `listener_play_history`, the settings
and the web UI with its three skins are all *above* the seam and do not notice.
Only the thing that turns a queue into sound is exchanged.

---

## 3. What a handoff does

**`[SPEC-BK-030]` The outgoing backend stops sounding; the incoming one starts.**
Both may hold a PipeWire stream at once, so the changeover is a **crossfade**
rather than a gap — the same `[REQ-AUD-158]` shape a skip already uses, and the
reason it is available is the first measurement above.

**`[SPEC-BK-035]` Position and queue transfer, as far as they can.**

| moving | the current passage | the rest of the queue |
| :--- | :--- | :--- |
| **Vaino → MPD** | `addid` + `rangeid`, then `seekid` to the audible position | re-offered as spans; exact |
| **MPD → Vaino** | resolved by URI, then played from position | adopted where nameable |

Vaino → MPD is exact because Vaino knows what it holds. **MPD → Vaino is not,
and must not pretend to be**: 191 of 5,709 files carry more than one radio
passage, and a whole-file entry could be any of up to forty of them. Those are
reported, never guessed `[SPEC-MPD-060]`.

> **This asymmetry matches what is asked of it.** Seamlessness is wanted
> Vaino → MPD and merely desirable MPD → Vaino, which is the direction that
> cannot be exact. The requirement and the constraint agree, which is luck worth
> noticing rather than design.

**`[SPEC-BK-040]` What a listener loses by moving to MPD is stated before they
move, not discovered after.** `Capabilities::MPD` drops `gain` and `ramps`: per
passage gain cannot be expressed at all, and lead-in/lead-out degrade to one
global crossfade number against a median lead-out of 946 ms `[SPEC-SC-043]`. The
control that offers the switch says so, once, in those terms.

---

## 4. What this removes from the appliance plan

**`[SPEC-BK-050]` The conversion is cancelled.** IMPL004 treated adding MPD as
turning a Vaino-plays box into an MPD-plays box `[IMPL-MPD-009]`. With one
process and two backends, the appliance stays a Vaino appliance that can *also*
be driven by MPD clients. Three problems that plan carried simply do not arise:

1. **Power-up resume survives.** `[PI5-PWR-030]` requires the appliance to
   resume playing if it was playing, and `[SPEC-MPD-120]` says the Director
   cannot start a session from cold. Under one process the local engine resumes
   as it does today, and MPD is a backend adopted later — never the thing that
   has to start.
2. **The settings page survives**, because the process serving it never stopped.
3. **The audio path supervisor survives.** `[PI3-API-010]`'s reopen-on-default-change
   was built and proved on hardware; keeping Vaino resident keeps it, instead of
   asking MPD to rediscover it.

---

## 5. Open

1. **`[SPEC-BK-060]` Whether MPD runs resident or on demand.** MPD is not
   installed on the appliance today. Resident costs memory that measurement says
   is tight; on-demand costs an indexing pass at switch time. Measure the index
   over 5,705 files before choosing.
2. **`[SPEC-BK-065]` What a crossfaded handoff does to the rotation ledger.**
   Two backends sounding at once for a second is two passages in flight, and
   `[SPEC-PLAY-010]` judges one at a time. Probably: the outgoing passage is
   judged at the moment it stops sounding, exactly as a skip is.
3. **`[SPEC-BK-070]` Whether an unnameable MPD entry blocks the switch or is
   dropped from it.** Dropping loses a person's choice; blocking makes 191 files
   able to veto a handoff. Likely: adopt what is nameable, report the rest, and
   let the person decide — but that is a decision, not a default.

---

**Traceability:** `[SPEC-BK-010..070]` · corrects `[IMPL-MPD-009]` · uses
`[GDE-BAK-040]`'s trait · constrained by `[PI5-PWR-030]`, `[PI3-API-010]`
