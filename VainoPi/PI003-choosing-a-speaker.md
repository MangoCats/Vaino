# PI003: Choosing a Speaker

**Design — Tier 2**

How a listener picks their Bluetooth speaker from the Vaino settings panel, and
why the obvious implementation of that is broken.

> **Related:** [PI001 partitions](PI001-image-and-partitions.md) ·
> [PI002 test image](PI002-test-image-setup.md)

---

## 1. What this has to survive

Every requirement below is the residue of a specific failure on real hardware,
recorded in [PI002 §6a](PI002-test-image-setup.md). None of them is
hypothetical, and the naive version of this feature reproduces all of them.

**`[PI3-WHY-010]` PipeWire always offers a `Dummy Output`.** When no sink is
present it invents one, and it is a perfectly good sink: a player attached to
it plays flawlessly, forever, into nothing, reporting itself healthy from every
angle. Two days of this problem were spent looking at Bluetooth because the
player insisted it was fine.

**`[PI3-WHY-020]` A stream sometimes follows a change of default sink, and
sometimes does not.** This is the awkward one, and the reason the fault took so
long to place.

Tested deliberately: with the speaker untrusted and disconnected, the player
starts and binds to `Dummy Output`. Reconnecting the speaker then relinked the
stream to MIDDLETON **by itself**, same node ids, no reopen. But the original
failure was precisely a stream sitting on `Dummy Output` while the speaker was
connected, staying there, and playing to nobody.

Both were observed on the same machine. So the relink is real but not
dependable -- plausibly a race between the dummy being removed and the sink
appearing -- and a design that relies on it works most of the time, which is
the worst frequency for a fault to have.

**Therefore "select a speaker" must reopen the output rather than change the
default and hope.** Not because the stream never follows, but because it
cannot be trusted to, and a cosmetic selection is silent.

**`[PI3-WHY-030]` A2DP dies when nothing feeds it.** A speaker with no audio
hangs up after a few seconds, and the listener hears a disconnection tone. This
makes every silent failure above *sound like* a Bluetooth fault, which is
exactly the wrong place to look.

**`[PI3-WHY-040]` `pair` and `trust` are separate, and only `trust` survives a
reboot.** Pairing without trusting yields a speaker that works beautifully until
the appliance restarts, then never reconnects.

**`[PI3-WHY-050]` A stale trusted-but-not-paired record blocks re-pairing
silently.** BlueZ keeps the entry, `pair` fails, and nothing explains why. The
fix is to `remove` the device first, which no listener will ever guess.

**`[PI3-WHY-060]` Pairing needs a registered agent.** Without
`agent NoInputNoOutput` the exchange never completes, and the failure is a
timeout rather than a message.

---

## 1a. What the listener should experience

**`[PI3-AIM-010]` Choose a speaker once. It is remembered. Pressing play
plays.** That is the whole requirement, and everything else in this document is
machinery in service of it. Written down because the parts can each work while
the experience still does not.

What that costs, beyond what is already built:

- **`[PI3-AIM-020]` Done, 2026-08-27.** The choice must be stored by Vaino, not
  inferred from BlueZ trust and PipeWire's default -- those agree only until a
  second speaker is ever tested. `use`/`pair` now write the address to
  `player_settings` `[REQ-VIS-260]`. Found by its absence: `vaino-speaker`
  (below) had been paging a hard-coded leftover from the `[PI3-WHY-020]`
  testing (`MIDDLETON`) every 30 s regardless of what was connected --
  stalling whatever *was* playing for several seconds each time, since paging
  an unreachable device ties up the one shared radio. Audible as a skip with
  the on-screen position frozen; invisible to the underrun counter, since the
  stall is on the radio and never touches the output ring.
- **`vaino-speaker`,** the timer that connects the stored speaker if absent and
  tells the player to reopen, now reads that stored address rather than a name
  compiled into the script. `SPEAKER` still overrides it for a library with no
  chosen speaker yet.
- **`[PI3-AIM-030]` Play must be willing to go and get the speaker.** The timer
  covers "off at boot." Still open: pressing play in the narrow window before
  its next tick still finds a dummy and correctly reports silence.
- **Failure has to stay legible.** A speaker that is off, flat, or in pairing
  mode cannot be reached by any amount of retrying, and the panel should say
  which of those it looks like rather than spinning `[PI3-UI-010]`.

## 2. The shape of it

**Built, but not yet seen.** The panel is written and serves correctly; it has
not been rendered in a browser by anyone. Treat the layout as unverified.

**`[PI3-UI-010]` One list, showing what is true.** The settings panel shows
every known and discoverable speaker in a single list, each with a state that
is *observed* rather than assumed:

| State | Meaning | Offered action |
|---|---|---|
| `playing` | connected, and Vaino's stream is attached to it | Forget |
| `connected` | linked, but audio is going elsewhere | Use this one |
| `paired` | known, not currently linked | Connect |
| `found` | in range, never paired | Pair |
| `stale` | trusted but not paired `[PI3-WHY-050]` | Repair |

The distinction between `playing` and `connected` is the entire lesson of
`[PI3-WHY-010]` made visible. A user who sees "connected" and hears nothing has
been told something useful; a user who sees a checkmark has been lied to.

**`[PI3-UI-020]` Selecting a speaker performs the whole sequence.** Connect,
trust, make default, **and reopen Vaino's output** -- the last step being the
one `[PI3-WHY-020]` makes mandatory and the one nobody would think to expose.
A selection that stops short of it appears to work and is silent.

**`[PI3-UI-030]` Confirm or revert, as for the network settings
`[PI-SET-014]`.** Switching speakers can destroy the means of hearing whether
the switch worked. So the new speaker is adopted provisionally, the panel asks
"can you hear this?" over a test tone, and an unanswered prompt returns to the
previous sink after 30 seconds. The listener cannot strand themselves in
silence by choosing wrongly.

**`[PI3-UI-040]` Pairing is a mode, not a button.** The panel offers "put my
speaker in pairing mode, then press this", scans for the ~20 s a speaker takes
to appear, and registers an agent for the duration `[PI3-WHY-060]`. Discovery
stops afterwards, because a permanently scanning radio is a permanently
degraded one on a shared antenna.

---

## 3. What the player must provide

**`[PI3-API-010]` Reopen the output on demand. Built and proven on hardware.**
`POST /command/reopen-output` rebuilds the stream against the current default
sink, keeping the same ring. Verified by the output node id changing and by
`pw-top` showing the rebuilt node running at 1102 quantum, 44100, F32LE 2ch,
with playback continuing across the reopen. A reopen that lands on a device not
ready yet -- a speaker still completing its connection is the normal case --
hands itself to the retry loop rather than failing once.

**`[PI3-API-020]` Report the sink actually in use. Built.** `GET /audio/sink`
answers from PipeWire, because ALSA only ever tells the player `default`.
Queried on demand rather than polled: it costs a subprocess, and a player that
shelled out every state tick would spend more effort describing its output than
producing it. `known: false` distinguishes "the query could not run" from "it
ran and found nothing", since the remedies differ. Verified on hardware, both
ways round:

    speaker connected  {"sink":"MIDDLETON","dummy":false,"known":true}
    speaker removed    {"sink":"Dummy Output","dummy":true,"known":true}

**`[PI3-API-030]` Never settle for a dummy, and notice when one arrives.
Built.** Two paths, because the first one shipped covering only half the
problem. The loud failure -- a stream that breaks -- is caught by the error
callback. The quiet one is a speaker switched off during normal playback:
PipeWire moves the stream to the `Dummy Output` and **reports no error at
all**. Nothing in the player is wrong at that moment. The callback runs, the
ring drains, the clock advances, and nobody can hear a thing. A guard that
fires only on reopen therefore almost never fires. So the engine also confirms,
every twenty seconds while playing, that the audio still reaches something
real.

**`[PI3-API-030]` (original) Never settle for a dummy.** `vaino-wait-sink` guards
boot; the engine now guards every reopen and every recovery. Opening
successfully says nothing about whether anyone can hear it -- the dummy accepts
audio perfectly -- so a reopen that lands there is marked failed and the retry
loop keeps looking. The player will not report itself recovered into silence.

**`[PI3-API-050]` Routes fronting the helper. Built.** `GET /audio/speakers`
lists; `POST /audio/speakers/:verb/:address` acts. The verb is an enum rather
than a string passed through, so an unknown one cannot reach the helper, and
the address is validated again in the player -- the helper's check is the one
that must not be bypassed, this one makes a malformed address a 400 with an
explanation rather than a non-zero exit nobody reads.

`use` reopens the output **as part of the same request** `[PI3-UI-020]`. Doing
it server-side rather than in the browser means no caller can forget the step
that makes the choice audible.

Every reply carries `audible`, which is the question a listener actually has.
It is `null` rather than `false` when the stream is not linked yet: "we could
not tell" and "it is not working" want different responses, and collapsing them
is how a fault stays hidden. The reply also waits for the reopen to land before
reporting -- reading immediately returned `sink:null` with `dummy:false`, which
reads as healthy and was merely early.

Choosing a speaker from silence, in one call:

    before  {"sink":"Dummy Output","dummy":true,"known":true}
    POST    /audio/speakers/use/20:64:DE:CF:F3:AD
    reply   {"audible":true,"state":"connected","reopened":true,
             "output":{"sink":"MIDDLETON","dummy":false,"known":true}}

**`[PI3-API-040]` Surface the recovery count `[REQ-VIS-140]`.** A link that
drops and recovers repeatedly is a range or battery problem, and a number
climbing in the diagnostics is how anyone would know. Silent recovery is right
for one dropout and wrong for fifty.

---

## 4. Privilege

**Built: `vaino-btctl`.** A closed set of verbs -- `list`, `scan`, `pair`,
`repair`, `use`, `forget`, `status` -- each taking at most a device address,
checked against an anchored MAC pattern *before* it reaches BlueZ. Nothing is
interpolated into a shell. It emits JSON so the caller parses a shape rather
than prose, and it is reached through a sudoers rule naming that one binary
rather than by granting the player broader rights.

Three verbs encode failures rather than operations. `pair` registers an agent
and paces its steps, because piping them as one block runs them faster than
bluetoothd establishes the connection `[PI3-WHY-060]`. `repair` removes the
device first, because a stale trusted-but-not-paired record makes every attempt
fail with no explanation `[PI3-WHY-050]`. `use` trusts as well as connects,
because trust is what survives a reboot `[PI3-WHY-040]`.

The sudoers file is validated with `visudo -c` and **removed if malformed** --
a bad one can lock the machine out of sudo entirely, and a setup script that
bricks its own escape route is worse than one that does nothing.

Verified on hardware. Injection and malformed addresses are refused
(`{"ok":false,"error":"not a device address"}`), unknown verbs are refused, and
the whole `[PI3-UI-020]` sequence runs from a silent start:

    before   {"sink":"Dummy Output","dummy":true,"known":true}
    use      {"ok":true,"state":"connected","sink_node":"46"}
    reopen   HTTP 204
    after    {"sink":"MIDDLETON","dummy":false,"known":true}

with `pw-top` confirming the node running at 1102 quantum, 44100, F32LE 2ch.

---

## 4a. The reopened stream, and feeding silence while paused

**A reopened stream was found to die where a fresh one does not — investigated
2026-08-16, improved, not proven closed.** What was found, ruled out, and
fixed is history: see
[PI008 §2](PI008-appliance-bringup-history.md#2-a-reopened-stream-dies-a-fresh-one-does-not).
Whether it is now fully closed is a still-open question, tracked in
[ROADMAP §4](../docs/ROADMAP.md#4-the-appliances-still-open-speaker-questions)
rather than here.

**`[PI3-OPEN-020]` Feed silence while paused. Built.** McRhythm did this, and
the same reasoning applies: A2DP tears down when nothing feeds it `[PI3-WHY-030]`, so a
paused Vaino loses its speaker after a few minutes and resuming needs a
reconnect. Pausing should stop the *music*, not the stream. This also likely
reduces how often the fragile reopen path is needed at all, which makes it
worth doing regardless. Pausing now silences the callback rather than stopping
the device: the ring is left untouched so resuming is still instant
`[REQ-AUD-142]`, and the silence is not counted as an underrun, because
inflating the one diagnostic that matters most to hide an intended quiet would
be its own small lie.

## 5. Deliberately not now

**`[PI3-NOT-010]` (superseded) Interference was not being designed around.** The dark arm of
`radio-silence-test.sh` exists to answer that question and has not been run,
because with Wi-Fi up the link now measures 40/40 across two minutes with audio
flowing. Should unexplained connection problems appear, that test is the tool
to reach for. Building mitigations for a problem not yet observed would be
designing against a guess.

**`[PI3-NOT-020]` Multiple simultaneous sinks.** One speaker, chosen. Whole-house
audio is a different product.

---

**What operating it taught** is in [PI004](PI004-speaker-operation.md): this
document says what the link must do, that one says what it did.
