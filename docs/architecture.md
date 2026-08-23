# System Architecture

**Orientation — Tier 1 · describes what is built, as of 2026-08-22**

How Vaino is put together: the components, the interfaces between them, and the
few rules that decide where a new piece belongs. Written against the tree rather
than against a plan — where something is designed but not built, it says so.

> **Related:** [SPEC018](spec/SPEC018-switching-backends.md) for the backend seam ·
> [SPEC016](spec/SPEC016-mpd-protocol-findings.md) for what MPD will and will not carry ·
> [SPEC017](spec/SPEC017-what-counts-as-a-play.md) for the listening rules

---

## 1. Two programs, one database

| | language | licence | writes |
| :--- | :--- | :--- | :--- |
| `player/` | Rust | MIT | listener state only |
| `tools/` — "Sampo", 38 scripts | Python | AGPL-3.0 | reference data only |

Everything that plays, or is played to, is in `player/`. Everything that *makes*
reference data — ingest, identification, flavor extraction, lyrics import — is
Sampo's. **The player never writes what Sampo owns**, for the reason
`[SPEC-LYR-030]` gives about lyrics and which generalises: derived data has one
writer, and a player that edits it becomes a second source of truth for
something it only reads.

---

## 2. The spine

```
 browser ──ws snapshot──┐        ┌─ Controls (intent cells) ─┐
                        ▼        ▼                           │
   web.rs  ──Command──▶ EngineHandle ──▶ vaino.rs loop ───────┘
                                            │
                                    Session (session.rs)
                                       │         │
                              Director │         │ dyn Backend
                                       ▼         ▼
                          director/library.rs   Switching (switch.rs)
                                                  ├── Engine      (local)
                                                  └── MpdBackend  (guest)
```

`vaino.rs` owns one thread and everything audio-related on it: `cpal`'s stream is
not `Send`, so the engine is built and pumped where it lives. The tokio web
server touches playback only through the two channels above.

**Two channels, and the difference matters.** A `Command` down `EngineHandle`
reaches the **local engine** and nothing else. An intent written into `Controls`
is picked up by the loop, which can reach whichever backend is live. Anything
that must act on the *sounding* side — a switch, a seek, a folder-writing
setting — is an intent cell; getting this wrong makes a control that silently
moves the wrong player.

---

## 3. The backend seam

Four traits, deliberately separate, in `playback.rs` and `switch.rs`:

| trait | methods | who wants it |
| :--- | :--- | :--- |
| `Playback` | `capabilities`, `enqueue`, `queued_ids`, `queued_ms`, `shortfall`, `take_dropped`, `resume_at`, `seek_to`, `tick`, `is_shutdown` | ordinary playback |
| `FadeOut` | `fade_out`, `hand_off` | a handoff |
| `Publish` | `publish` | a guest's clients |
| `Progress` | `head_position`, `refresh`, `head_counted`, `adopt_counted` | a handoff |
| `Backend` | all four | `Switching` |

Fading, publishing and position are things a **handoff** wants and ordinary
playback never does. Widening `Playback` for one caller is how a seam stops
being the shape of what crosses it `[SPEC-BK-020]`.

**`Switching` is itself a `Backend`**, forwarding to whichever side is live, so
`Session` drives one thing and never learns it changed. `Capabilities` is
reported from the **live side only, never the union** — reporting `FULL` while a
guest plays would promise gain and ramps MPD cannot honour `[SPEC-BK-040]`.

**What crosses a handoff is passage ids and a position**, never audio and never
decoder state. Spans, gain and ramps are re-derived from the library on arrival,
because they belong to the passage and not to whichever backend last played it.

---

## 4. The audio path

```
Director ─▶ QueueEntry ─▶ queue.rs ─▶ decoder.rs ─▶ resample.rs ─▶ mixer.rs ─▶ output ring ─▶ path.rs ─▶ device
                                     (symphonia,     (rubato)     (per-passage    (~14 s)     (supervisor,
                                      seeks to span)               gain, ramps)                owns the device)
```

`QueueEntry` is the unit every layer speaks: `passage_id`, `path`,
`start_ms`/`end_ms`, `lead_in_ms`/`lead_out_ms`, `gain_db`, `mbid`, naming.

**A passage is a span of a file**, which is why a whole-file backend cannot carry
`kind='radio'` trim points, and why one capture holds forty passages with one set
of tags.

Three things worth knowing before changing anything here:

* **`path.rs` is a supervisor on its own thread** and owns the device. A sink
  that vanishes — a Bluetooth speaker walking out of range — is reopened rather
  than fatal `[SPEC-APS-060]`.
* **The ring holds about 14 seconds.** Anything the listener asks for *now* —
  skip, seek — must cut the ring back, or it arrives 14 seconds later. There is
  one place that does this and both callers share it.
* **Gain is applied per passage, before the mix**, so each side of a crossfade
  carries its own level. Applying it after would level the blend rather than the
  tracks.

---

## 5. The Director

`director/` splits selection into `library` (the pool, and `decide`), `frequency`
(suppression windows and rotation), `flavor` (acoustic distance), `occasion`,
`shape` and `program`. It returns an `Explanation` with every choice, which is
what `/why` shows and what a guest's clients read as an MPD sticker.

**It is the same `decide` call whichever backend is sounding.** The Director has
no idea whether its choice will be decoded locally or handed to MPD.

---

## 6. Data

`db.rs` is the only gateway to SQLite. Two classes of data, and the distinction
is load-bearing `[SPEC-DF-055]`:

* **Class C — reference data** (recordings, artists, releases, flavor, lyrics,
  cover art). Derived, reproducible, and it **travels** between installations.
* **Class D — listener state** (plays, rejections, resume point, settings,
  programmes). Personal, irreplaceable, and it **never travels**.

Identity is `audio_md5` > `recording_mbid` > `file_path` `[SPEC-DF-030]`. That
ladder is why a lyrics import is a join rather than a matching problem, and why
a library that moved on disk can be relinked at all.

---

## 7. The interface

One `Snapshot` struct, serialised to JSON and pushed over `/ws`; REST for
actions. Three skins — `vaino`, `mulibplay`, `winamp` — served from
`web/skins/`, sharing `core.js`.

**`core.js` owns anything two skins would otherwise implement twice**: the queue
edit verbs, the seek arithmetic, art and lyrics fetching, formatting. A skin
styles and places; it does not decide what a control does.

A test asserts the snapshot's **field names**, because renaming one silently
blanks part of every skin and no Rust test would otherwise catch it.

---

## 8. Where the audio comes out

> **Rule**: *where the Vaino server runs is where the audio comes out.*

Vaino is not a network audio server. The web interface is a control and
visualisation plane; sound leaves the host's own hardware — a DAC hat, a
Bluetooth pair, a sound card.

**The MPD backend does not change this.** MPD is a *guest on the same machine*:
Vaino hands it passages, and MPD plays them out of the same host's audio. What
moves over the network in that arrangement is control, not sound.

---

## 9. Conventions that hold everywhere

* **Every tunable default is defined once in `lib.rs`** and referenced —
  `SKIP_SUPPRESS_H`, `QUEUE_DEPTH`, `SAMPLE_INTERVAL_MS`. A number that appears
  twice will diverge.
* **A measured constant carries its measurement** in the doc comment beside it.
  The comment is where the reasoning lives; a threshold with no number behind it
  is a preference pretending to be a finding `[GOV-SRC-040]`.
* **Anything written outside Vaino's own storage is off by default** and asked
  for explicitly — cue sheets, cover art, lyrics `[REQ-VIS-205]`.
* **Reports name what they left out.** A count is not a report; a shortened
  queue that says nothing about being shortened is the failure `[PI3-API-030]`
  exists to refuse.

---

## 10. Known gaps

* **The UI is blank while MPD is the live backend** — title, position and
  duration come from the local engine's published state, so the seek bar is not
  offered there.
* **MPD resident versus on-demand is undecided** `[SPEC-BK-060]`: it is not
  installed on the appliance at all, and everything MPD-related has only ever
  run on a Windows host.
* **No integration tests.** `player/tests/` is empty; coverage is unit tests
  inside modules, so cross-module invariants — cue numbering against
  `cue_uris`, for one — are held by comments rather than by tests.
* **Four folder-writing modules repeat one shape** (`cue`, `covers`,
  `lyrics_cache`, `lyrics_sidecar`), with a report vocabulary that has drifted
  between them.

---

**Traceability:** describes `[SPEC-BK-020..065]`, `[SPEC-DF-030]`,
`[SPEC-DF-055]`, `[SPEC-APS-060]` · supersedes the pre-implementation sketch of
2026-07-26
