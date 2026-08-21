# SPEC015: The Director as an MPD Client

**Design Specification — Tier 2 · PROVISIONAL, on `investigate/external-players`**

How Vaino's Program Director `[SPEC009]` reaches the MPD ecosystem: what MPD already does better than reimplementing it, and what it cannot do that Vaino must supply.

> **Related:** [GUIDE007](../GUIDE007-external-backends-investigation.md) measured the cost · [GUIDE006](../GUIDE006-director-as-a-guest.md) posed it · [SPEC013](SPEC013-sampo-console.md) for the two-UI precedent

---

## 1. Shape

**`[SPEC-MPD-010]` It is a client, because MPD has no seam for anything else.** The protocol documentation is explicit that there is no plugin interface for deciding what plays next; queue order is a client's business, and `prio`/`prioid` only reorder within random mode. So the Director sits beside MPD and feeds it.

```
   ncmpcpp · M.A.L.P. · myMPD · Cantata · Shinobu          ← ~60 existing clients
                        │  control playback as usual
                        ▼
                      MPD        ← plays files, owns the queue, owns the audio
                        ▲
        idle · addid · rangeid · sticker · currentsong
                        │
                   vaino-mpd     ← the Director, headless, no audio path
                        │
                        ▼
                    vaino.db     ← flavor, history, rotation  (Sampo unchanged)
```

**`[SPEC-MPD-015]` This is an established shape in the ecosystem, not a novelty.** MPD's own client list carries a *utility / non-interactive* category, and `MPD_sima` already occupies it as an auto-queueing client. Vaino would be another of those, with a better answer to the same question.

**`[SPEC-MPD-020]` The payoff is the client ecosystem, and it is the real reason to do this.** Vaino gains around sixty maintained front-ends — terminal, web, desktop, Android, Wear OS, iOS — without writing any of them, and gains them on **existing installations**. A moOde or Volumio owner adds selection to a box they already have `[GDE-BAK-080]`; nobody is asked to adopt an appliance image.

---

## 2. What MPD is left to do

**`[SPEC-MPD-030]` Everything it is already good at, unmodified.**

| Concern | MPD command | Note |
| :--- | :--- | :--- |
| Notice a track ended | `idle player playlist options` | event-driven; no polling loop |
| Add a chosen passage | `addid <uri>` → song id | |
| **Play a passage, not a file** | **`rangeid <id> <start:end>`** | fractional seconds, since 0.19 `[GDE-BAK-035]` |
| Retire what has played | `consume 1` | MPD removes it; the queue becomes pure lookahead |
| Observe what is playing | `status`, `currentsong` | elapsed and duration, so a skip is distinguishable from a play |
| Audio output, formats, multi-room | its own | Vaino contributes nothing and should not |

**`[SPEC-MPD-035]` `consume 1` makes MPD's queue the same object as Vaino's.** Vaino keeps a depth of five ahead; with `consume` on, the MPD queue drains as it plays and `vaino-mpd` tops it back up. The refill rule is the one already written — `[SPEC-DIR-160]`'s flow is measured from the tail, and the queued ids are the exclusion set.

---

## 3. What Vaino supplies that MPD has not got

**`[SPEC-MPD-040]`** In descending order of how badly MPD lacks it:

1. **Flavor.** MPD has no acoustic notion of any kind. The 71-dimension vector, its provenance and its measured accuracy are Vaino's `[SPEC-SC-060]`.
2. **Rotation, recovery, restraint.** MPD offers `random`, `repeat`, `single`, `consume` — none time-aware. Six years of tuned frequency behaviour has no MPD counterpart `[GDE-PD-010]`.
3. **The Album/Radio duality.** MPD stores no per-song trim. Vaino holds `start_ms`/`end_ms` and applies them per add via `rangeid`, which is why this target keeps what 98.6% of the library is shaped by `[GDE-BAK-030]`.
4. **Long-term play history.** MPD does not keep one. Vaino's `listener_play_history` does, and it is the only irreplaceable data in the system `[SPEC-DF-090]`.
5. **Why this track.** No equivalent — see below.

---

## 4. Extending MPD without patching MPD

**`[SPEC-MPD-050]` The sticker database is the extension point, and it is a real one.** MPD attaches arbitrary name/value pairs to a song URI, persists them, and `sticker find` supports `==`, `<`, `>`, `contains` and `starts_with`. Nothing needs to be added to MPD for Vaino to publish into the world MPD's clients already read.

Proposed namespace, all under `vaino.`:

| Sticker | Value | Read by |
| :--- | :--- | :--- |
| `vaino.passage` | passage id in `vaino.db` | `vaino-mpd` itself — see `[SPEC-MPD-060]` |
| `vaino.why` | the weight decomposition, JSON `[REQ-VIS-100]` | any sticker-aware client |
| `vaino.flavor` | a short human summary, e.g. `danceable · not sad` | any sticker-aware client |
| `vaino.chosen_at` | unix seconds | `sticker find` for "what did it pick today" |

**A client that knows nothing of Vaino is unaffected**; one that shows stickers gains the "why this track" panel for free. That is the whole of the extension, and it requires no MPD change, no fork, and no protocol addition.

**`[SPEC-MPD-055]` What cannot be expressed, stated rather than approximated.**

| Vaino has | MPD has | Verdict |
| :--- | :--- | :--- |
| per-passage `gain_db` `[SPEC-SC-040]` | `replay_gain_mode`, **per file** | **cannot express** — two passages in one file may differ |
| `lead_in_ms` / `lead_out_ms`, per passage | `crossfade` (global seconds), `mixrampdb` | **approximation only** — the median lead-out is 946 ms `[SPEC-SC-043]` and a global crossfade is one number for every track |

Both are declared through `Capabilities` `[GDE-BAK-040]` rather than silently dropped: `Capabilities::MPD` is `{ spans: true, gain: false, ramps: false }`. A run that cannot honour gain says so once at startup, not never.

---

## 5. The mapping, which is the hard part

**`[SPEC-MPD-060]` Vaino names a passage; MPD names a URI relative to its own `music_directory`. Nothing guarantees the two libraries agree** — this is `[SPEC-RLK-025]`'s lesson in a new costume, and GUIDE007 named it the piece to prototype first `[GDE-BAK-025]`.

Resolution order, narrowest first, in the spirit of `[SPEC-DF-040]`:

1. **Same tree.** Where MPD's `music_directory` is a prefix of `files.path`, the URI is the remainder. Cheap, exact, and the common case for an appliance where both were configured by the same person.
2. **MusicBrainz recording id.** MPD reads and exposes `MUSICBRAINZ_TRACKID` where files carry it; Vaino holds recording MBIDs. Survives different trees, fails on the 164 recordings here that have no MBID at all.
3. **Ask a person.** Anything unresolved is reported, never guessed. A wrong mapping plays a real song under a real name and nothing downstream can tell — the exact failure `[REQ-LIB-165]` exists to correct.

**`[SPEC-MPD-065]` The resolved mapping is cached in MPD's own sticker database**, as `vaino.passage` on the URI, and **not** in `vaino.db`. Three reasons: the mapping is a property of *that MPD instance* rather than of the library; a second MPD gets its own without collision; and a Vaino user who never touches MPD grows no table they will not fill `[SPEC-SC-015]`.

---

## 6. Containment

**`[SPEC-MPD-070]` A separate binary behind a default-off feature.** `player/src/bin/vaino-mpd.rs`, gated by `--features mpd`, using `std::net` and no new dependency. The appliance build is byte-identical to today's `[REQ-HW-140]`, and a user who does not want this never compiles it.

**`[SPEC-MPD-075]` Vaino's own UI keeps its job.** MPD clients drive playback; Vaino's web UI stays for what they cannot show — browse with provenance, the review queue, the why panel `[REQ-VIS-180]`. The same division SPEC013 draws between the console and the player: two surfaces, two questions, one database.

**`[SPEC-MPD-080]` Target MPD 0.19's protocol surface, not the newest.** `rangeid`, `sticker`, `idle` and `consume` have all been present since 2016. Stable-distro users run years-old versions by policy rather than choice `[GDE-BAK-120]`, and depending on anything newer would exclude half the user base for years.

---

## 7. Settled

**`[SPEC-MPD-090]` A play is a play by the scrobbling rule: half the track, or
four minutes, whichever comes first.** *(Decided 2026-08-21.)* Both Last.fm and
ListenBrainz use exactly this threshold, and adopting it rather than inventing
one buys three things: it is already tuned by services that have watched
billions of listens, it will agree with whatever scrobbler the user is running
`[SPEC-MPD-100]`, and it is a number a person can check.

**One deviation, deliberate.** Last.fm additionally ignores tracks under 30
seconds and ListenBrainz under 5. That floor is an *anti-spam* rule about
fraudulent submissions to a public service, and it does not apply to a private
rotation ledger. Vaino's shortest radio passage is **12 seconds**
`[SPEC-SA-090]`; one that played in full did play, and excluding it would
suppress nothing but the truth. **The threshold is adopted; the minimum-length
exclusion is not.**

> **Mechanically, elapsed must be sampled rather than read at the end.** `idle
> player` fires *after* the change, when `currentsong` and `elapsed` already
> describe the new song, and `consume 1` removes a skipped song exactly as it
> removes a finished one. So `vaino-mpd` polls `status` at a low rate while
> playing and keeps the last known elapsed, then judges the outgoing passage
> against it. This is the one place the design polls, and it is why.

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

**`[SPEC-MPD-094]` A stop ends a passage; a pause does not.** MPD retains `songid`
across a stop, so a watcher keyed on the song identity alone never notices one — a
track stopped past its threshold went unrecorded, and stayed unrecorded if nothing
played after it. A pause is the opposite case: elapsed holds still, the listener
is coming back, and closing the book on them would count a play as a skip.

**`[SPEC-MPD-105]` Both tunables are the listener's, edited on the settings page
and remembered.** *(Decided 2026-08-21.)*

| Parameter | Default | Meaning |
| :--- | ---: | :--- |
| **queue depth** | **5** | how many passages the Director keeps ahead. **At or above it, the Director adds nothing** `[SPEC-MPD-095]`. |
| **status sample interval** | **5 s** | how often `status` is read while playing, to judge a play against `[SPEC-MPD-090]`'s threshold |

They follow the pattern the player already has for skip fade, skip lead and
resume-save `[REQ-VIS-155]`: written the moment a control moves rather than on a
timer, persisted in `player_state` beside the three columns already there, and
with their **bounds carried in the snapshot** so the control offers exactly what
the engine accepts rather than keeping a second copy of the limits.

**Queue depth is a promotion, not a new setting.** It exists today as
`vaino --depth N`, defaulting to 5, reachable only by editing a service file.
Moving it to the settings page makes it adjustable on an appliance whose only
interface is a web page, and it applies to the **local** engine as well — the
same number, one place, whichever backend is playing.

**The sample interval has a floor worth respecting.** `[SPEC-MPD-110]` is why:
five seconds resolves a four-minute rule easily and a 12-second passage badly,
so the useful range is small at the bottom and the cost of the default being
wrong is a misjudged play rather than a missed one.

**`[SPEC-MPD-095]` The queue belongs to whoever is in front of it. The Director
only ever adds, and only ever below the minimum depth.** *(Decided 2026-08-21.)*
MPD's queue is shared, and a person editing it is not an error to be corrected.

| A person… | The Director… |
| :--- | :--- |
| adds twenty tracks | **adds nothing** — the queue is above depth |
| reorders | leaves the order alone; reads the tail for flow `[SPEC-DIR-160]` |
| removes one of Vaino's picks | tops back up to depth, **with a fresh choice** |
| clears the queue | refills to the minimum, five `[SPEC-MPD-035]` |

**It never removes, never reorders, and never re-adds what was taken out.**
Putting a rejected pick back is the one behaviour that would read as the machine
arguing with the listener.

**And a removed pick must be un-counted.** `note_queued` marks a passage as
recently played so rotation suppresses it while queued; if a person deletes it
before it plays, `forget_queued` has to run or one deletion suppresses that
recording and its artist for a full rotation `[REQ-PD-112]`. Locally that is
driven by `take_dropped`; here the trigger is a queue diff after `idle
playlist`, and the two must reach the same bookkeeping.

**`[SPEC-MPD-100]` `vaino-mpd` does not scrobble.** *(Decided 2026-08-21.)*
MPD's ecosystem already carries scrobblers — `mpdscribble`, `mpdas`, and several
clients — and a second submitter would duplicate every listen. Vaino writes its
**own** `listener_play_history`, which is a different ledger for a different
purpose: rotation input, not a public record `[SPEC-DF-055]`. Sharing the
threshold with the scrobblers `[SPEC-MPD-090]` is what keeps the two agreeing
about what happened without either writing the other's data.

---

## 8. Open

1. **`[SPEC-MPD-110]` Whether one interval can serve both halves of the rule.** Five seconds `[SPEC-MPD-105]` judges a four-minute threshold to within 2% and a 12-second passage to within 40%. The floor may need to follow the passage rather than the clock — sampling near the half-mark of whatever is playing — which is a smaller change than it sounds and should be measured before it is assumed necessary.
2. **`[SPEC-MPD-115]` Whether a person's own additions should feed rotation.** They played, so they happened — but they were not chosen by the Director, and counting them shapes future selection by a hand the algorithm cannot see. Probably yes, on the grounds that `listener_play_history` records listening rather than deciding.
3. **`[SPEC-MPD-120]` What "active" means for the Director** `[SPEC-MPD-095]`. There must be a way to leave it running but quiet, or a person browsing their library fights a daemon that keeps appending.

---

**Traceability:** `[SPEC-MPD-010..120]` · derived from `[GDE-BAK-035]`, `[GDE-BAK-025]`, `[SPEC-DIR-160]`, `[SPEC-SC-043]`
