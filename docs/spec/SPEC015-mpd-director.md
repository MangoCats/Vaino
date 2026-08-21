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

## 7. Open

1. **`[SPEC-MPD-090]` Whether a play can be told from a skip reliably enough for rotation.** `status` gives elapsed and duration, but the threshold is a judgement and rotation is the stage with six years of tuning behind it.
2. **`[SPEC-MPD-095]` What happens when a person edits the queue.** MPD's queue is shared: another client may clear it, reorder it, or add its own choices. Vaino must treat that as authoritative rather than fight it, and `[REQ-PD-112]`'s bookkeeping needs to survive passages vanishing for reasons that are not failures.
3. **`[SPEC-MPD-100]` Whether `vaino-mpd` should also write scrobbles**, or leave that to the clients already doing it.

---

**Traceability:** `[SPEC-MPD-010..100]` · derived from `[GDE-BAK-035]`, `[GDE-BAK-025]`, `[SPEC-DIR-160]`, `[SPEC-SC-043]`
