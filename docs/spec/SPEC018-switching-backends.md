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

**`[SPEC-BK-022]` The session drives a backend, not the engine.** *(Built
2026-08-21.)* `Session::refill`, `tend_rebuild` and `adopt` take
`&mut dyn Playback`. `Engine` satisfies the trait with nothing in `engine.rs`
touched, which was the spike's claim and survives contact.

**The trait had to narrow to carry weight, though.** As written it asked for
`queued() -> Vec<QueueEntry>` — a deep clone of every queued passage, per tick,
where the concrete code handed out borrows. The four call sites want three
**passage ids** and one **queued duration**, so it asks for those instead and
costs a `Vec<i64>`. A seam is only free if it is the shape of what crosses it.

**And the settings do not cross it.** `refill` takes the suppression windows as
an argument rather than asking the backend, because they are the listener's and
are the same whoever is playing. What stayed on `Engine` is exactly what should:
`apply_settings`, `attach_store` — process setup, not playback.

```
Capabilities::FULL  { spans: true, gain: true,  ramps: true  }   Vaino's engine
Capabilities::MPD   { spans: true, gain: false, ramps: false }   MPD as a guest
```

**`[SPEC-BK-027]` Both backends exist, and the session drives either.** *(Built
2026-08-21.)* `MpdBackend` implements the same trait: `enqueue` becomes `addid`
plus a verified `rangeid` `[SPEC-MPD-096]`, `tick` becomes a rate-limited poll,
`take_dropped` reports what MPD would not play, and `shortfall` returns **zero
unless MPD is playing** — so `[SPEC-MPD-120]`'s activation rule needs no special
case anywhere else in the session.

Demonstrated by *not* writing a loop. `mpd_session` opens the `Session` that
`vaino` runs, hands it an `MpdBackend`, and calls `refill`. Against a live MPD
it filled the queue to depth with Director selections — spans intact, including
`2312.672-2578.004` inside a capture — and the census moved as it does locally,
artist-blocked **0 → 180**. The Director, its rotation, its flow and its
bookkeeping reached a backend that is not the built-in one without knowing they
had.

**`[SPEC-BK-025]` Switching backend is not switching player.** The Program
Director, the library, the flavor index, `listener_play_history`, the settings
and the web UI with its three skins are all *above* the seam and do not notice.
Only the thing that turns a queue into sound is exchanged.

---

## 3. What a handoff does

**`[SPEC-BK-030]` The outgoing backend stops sounding; the incoming one starts.**
Both may hold a PipeWire stream at once, so the changeover *can* be a crossfade
rather than a gap — the same `[REQ-AUD-158]` shape a skip already uses, and the
reason it is available is the first measurement above.

**Built so far: the changeover, and an honest account of how it stopped.**
`switch_to_over` stops the outgoing side before switching — after would silence
the side just arrived at — and returns `Faded` or `Cut`. Neither side fades
unconditionally:

| side | can it fade? |
| :--- | :--- |
| MPD | **only with a mixer on its output** `[SPEC-MPD-099]`; measured `Cut` on a null output, expected to fade on PipeWire |
| Vaino's engine | **not yet.** `Fade`, its curves and `skip_fade_ms` all exist `[REQ-AUD-158]`; nothing drives them to silence on demand |

So today a switch is reliable and audible as a stop-then-start. Saying `Faded`
where it cut would be the failure `[PI3-API-030]` names, so the caller is told
which happened and the local engine reports `Cut` rather than flattering itself.
Driving the existing fade machinery to silence is the remaining audio-path work,
and it is deliberately not a line in a trait impl.

**`[SPEC-BK-032]` The queue crosses as passage ids, and is rebuilt on arrival.**
*(Built 2026-08-21.)* `Session::hand_over` switches the side, reads each carried
id back out of the library, and enqueues it into the incoming backend. **Spans
are re-derived rather than carried**: `start_ms`, `end_ms`, gain and ramps
belong to the passage, not to whichever backend last played it, and handing over
a built entry would have carried the outgoing side's idea of the passage into
the next one.

The transfer lives on the session because the session is the only thing holding
a library; a backend has no business holding one. A passage the library has
renumbered away since the queue was built is skipped and named, and the
Director's queueing mark for it is undone `[REQ-PD-112]` — it never played.

Demonstrated against a live MPD and a **real** `Engine` on a silent output: MPD
holding three Director-selected passages, `hand_over` to the local side, all
three rebuilt and queued there, and the reported capabilities flipping from
`gain false ramps false` to `gain true ramps true` in the same breath.

**`[SPEC-BK-035]` Position and queue transfer, as far as they can.**

| moving | the current passage | the rest of the queue |
| :--- | :--- | :--- |
| **Vaino → MPD** | `addid` + `rangeid`, then `seekid` to the audible position | re-offered as spans; exact |
| **MPD → Vaino** | resolved by URI, then played from position | adopted where nameable |

Vaino → MPD is exact because Vaino knows what it holds. **MPD → Vaino is not,
and must not pretend to be**: 191 of 5,709 files carry more than one radio
passage, and a whole-file entry could be any of up to forty of them. Those are
reported, never guessed `[SPEC-MPD-060]`.

**`[SPEC-BK-045]` An MPD entry Vaino cannot name is dropped from the switch,
not blocked by it.** *(Settled 2026-08-21.)* 191 of 5,709 files carry more than
one radio passage, so a whole-file entry can be unnameable through no fault of
the listener. Letting those veto a handoff would put a rare and invisible
property of the *library* in charge of an action a person just asked for.

The handoff therefore takes what it can name and leaves the rest behind,
**saying which** — a dropped entry is reported, because silently shortening
someone's queue is the kind of quiet wrongness `[PI3-API-030]` exists to
refuse. What is dropped is dropped from the *queue*, not from the library: the
passage is as playable as it ever was, and the next thing that names it will
get it.

> **This asymmetry matches what is asked of it.** Seamlessness is wanted
> Vaino → MPD and merely desirable MPD → Vaino, which is the direction that
> cannot be exact. The requirement and the constraint agree, which is luck worth
> noticing rather than design.

**`[SPEC-BK-037]` The passage that was sounding is judged as it stops, exactly
as a skip is.** *(Settled 2026-08-21.)* No new rule, and that is the finding:
`[SPEC-PLAY-010]` already asks whether enough was heard, and a handoff is just
another way for a passage to stop being heard. It is judged once, by the side
it was sounding on, against what that side observed.

This is only simple because **the sounding passage does not cross**. What
crosses is the queue `[SPEC-BK-032]`, and a queue does not include what is
already playing. Carrying the current passage as well would make it possible to
judge it twice — once as it stopped on one side, once as it ended on the other —
and the fix for that is not to carry it but to hand over what was already heard
along with it. That is the price of true mid-passage seamlessness, and it is not
paid here.

> **So a handoff costs the remainder of one passage**, and the crossfade is
> between that passage and the next one rather than within it. For MPD → Vaino
> this was named as acceptable. For Vaino → MPD it is the open edge of the
> design, and the honest version of "seamless" that is built today.

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
2. *(Settled — moved to `[SPEC-BK-037]` below.)*
3. *(Settled — moved to `[SPEC-BK-045]` below.)*

---

**Traceability:** `[SPEC-BK-010..070]` · corrects `[IMPL-MPD-009]` · uses
`[GDE-BAK-040]`'s trait · constrained by `[PI5-PWR-030]`, `[PI3-API-010]`
