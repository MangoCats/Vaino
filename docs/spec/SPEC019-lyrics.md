# SPEC019: Lyrics

**Design Specification — Tier 2 · storage and Vaino's own skins built; the guest half measured, not built**

MuLibPlay accumulated lyrics for a quarter of the library. This is how they
reach Vaino, its own skins, and — as far as the protocol allows — a guest's
clients.

> **Related:** [SPEC016](SPEC016-mpd-protocol-findings.md) for what MPD will and will not carry · [REQ-VIS-205](../spec/REQ002-functional-requirements.md), [REQ-VIS-210] for the two settings this follows

---

## 1. What exists

Measured against `MuLibPlay/mulib.db` and the live library, 2026-08-22.

| | |
| :--- | ---: |
| MuLibPlay tracks | 8,116 |
| …carrying lyrics | **2,288 (28.2%)** |
| …that also carry a recording MBID | **2,288 — all of them** |
| Recordings Vaino also knows | **2,265 (99.0%)** |
| Radio passages that would gain lyrics | **2,326 of 8,330 (27.9%)** |
| …of those, inside a DAO capture | 702 |

Plain text, not timestamped. Average 1,140 characters, longest 5,820.

**`[SPEC-LYR-010]` They key by recording MBID, which is why this is worth
doing.** Every lyric in MuLibPlay carries `mbidRecording`, and that is the
identity Vaino already transports `[SPEC-DF-030]`. The import is a join, not a
matching problem — no fuzzy titles, no heuristics, and 99% land.

---

## 2. Where they live

**`[SPEC-LYR-020]` A `lyrics` table keyed by `mbid`, beside `recordings`.**

```sql
CREATE TABLE lyrics (
    mbid       TEXT PRIMARY KEY REFERENCES recordings(mbid),
    text       TEXT NOT NULL,
    source     TEXT NOT NULL,     -- 'mulibplay', later others
    fetched_at TEXT NOT NULL
);
```

**Keyed by recording, not by passage.** A recording's words do not change
because it was ripped twice `[SPEC-DF-040]`, and two passages of one recording
share them. This is the same scope `flavor` and `recordings` already use.

**`[SPEC-LYR-025]` Class C: they travel.** Lyrics are derived reference data
about a recording, like a title or a flavor vector — not an account of one
person's listening `[SPEC-DF-055]`. A payload carrying a recording should carry
its words, and `[SPEC-PL-*]` gains a `lyrics` field on the recording object.

> At an average 1,140 bytes over 2,265 recordings this is about 2.6 MB — a
> rounding error beside the audio, and worth measuring again if a source with
> longer texts arrives.

**`[SPEC-LYR-030]` Import is Sampo's, not the player's.** `tools/import_lyrics.py`
reads a MuLibPlay database and writes `lyrics` rows. The player never writes
this table, for the same reason it does not write `recordings`.

---

## 3. Reaching Vaino's own skins

**`[SPEC-LYR-040]` A separate endpoint, not the snapshot.** `GET /lyrics/{passage_id}`
returns the words for that passage's recording, or 404. The snapshot is
published on every tick and read by every skin; 5.8 KB of text in it would be
sent hundreds of times to say what changes once a song.

The skin fetches on a change of passage, which it already notices.

**`[SPEC-LYR-045]` The MuLibPlay skin gets a panel; the others may.** The
endpoint is skin-neutral and every skin can adopt it, exactly as `build` was
made available to all three `[REQ-VIS-200]`.

---

## 4. Reaching a guest's clients

**`[SPEC-LYR-050]` MPD has no lyrics facility, and this must be said plainly.**
Measured: **no lyrics command among its 102**, and **no lyrics tag among its
34**. There is no "MPD lyrics interface" to implement against. Whatever a client
displays, it found by its own convention — a sidecar file beside the audio, an
embedded `USLT` frame, or an online lookup.

So this is the third instance of one shape `[SPEC-MPD-052]`: Vaino knows
something the protocol cannot carry, and the only route is to put a file where
the client will look.

**`[SPEC-LYR-055]` A sidecar, behind a third opt-in setting.** `<basename>.lyrics`
beside the audio file, written only when asked, never overwriting one Vaino did
not write, idempotent — the same three properties `[REQ-VIS-205]` and
`[REQ-VIS-210]` already keep, for the same reason: it writes into a folder Vaino
does not own.

**And the same limit, in the same place.** One file holds one sidecar, so a
capture cannot express per-passage lyrics: **1,624 passages are reachable this
way, 702 are not**.

**`[SPEC-LYR-060]` The timed `.lrc` idea is dropped, and the lookup order is
now known rather than guessed.** *(Measured 2026-08-22.)* Cantata's own
`context/songview.cpp` gives the sequence:

| | tried |
| ---: | :--- |
| 1 | `<audiofile>.lyrics` beside the music |
| 2 | `<audiofile>.txt` |
| 3 | `cache/lyrics/<artist>/<title>.lyrics` — **nested per artist** |
| 4 | `cache/lyrics/<artist>/<title>.txt` |
| 5 | `~/.lyrics/…` (not on Windows) |
| 6 | online |

Confirmed by experiment: a handwritten `<audiofile>.lyrics` displayed, as a
**static block** — which is how MuLibPlay showed them and how Vaino will. No
synchronisation is wanted, so the `.lrc` is unnecessary.

**The order is the design constraint.** The music-folder sidecar wins over the
cache, so the two routes are ranked rather than complementary:

* **Ordinary files** — one song each — take `<audiofile>.lyrics`. A convention
  other clients share, and portable.
* **Captures** cannot: one file, one sidecar, twelve songs. Per-passage words
  would have to go in the **cache**, keyed by the artist and title the cue sheet
  already supplies `[SPEC-MPD-056]`.

> **The cache route is not obviously Vaino's to take.** It means writing into
> another application's private directory, and it is **Cantata-specific** — no
> other client looks there. That is a different kind of intrusion from the music
> folder and deserves its own decision, not a fold into the same checkbox.

---

## 5. What this proposal does not do

- **No online lookup.** A missing lyric stays missing; nothing here reaches the
  network `[SPEC-MPD-100]`.
- **No embedding into audio files.** Rejected for cover art on the same grounds
  and rejected here more firmly: a re-tag that goes wrong is not recoverable
  from a database backup.
- **No editing.** Vaino displays what it was given. A correction belongs where
  the data came from.

---

**Traceability:** `[SPEC-LYR-010..060]` · transported under `[SPEC-DF-030]` ·
guest limits from `[SPEC-MPD-052]` · settings pattern from `[REQ-VIS-205]`
