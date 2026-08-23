# PI006: What the Appliance Actually Costs

**Measurement — Tier 1 · measured on vainopi, 2026-08-23**

The player as it runs on the hardware it is for: what it uses, what it can be
asked for, and the one thing it does badly. Every number here was taken from
the appliance itself rather than inferred from the development machine, which
has eight times the memory and no thermal limit worth the name.

> **Related:** [PI001](PI001-image-and-partitions.md) for the machine ·
> [PI005](PI005-appliance-library.md) for the library on it ·
> [IMPL001](IMPL001-appliance-setup.md) for how it was built

---

## 1. The machine and the build

| | |
| :--- | :--- |
| Hardware | Raspberry Pi, 4 cores, **464 MB RAM**, 511 MB swap |
| OS | Linux 6.12.93 aarch64, Raspberry Pi OS |
| Storage | 235 GB, 22% used; library 1.07 GB |
| Build under test | `vaino 0.1.0 (b8bb2e29b94e)`, cross-compiled aarch64, 7.4 MB |
| Library | 8,330 radio passages, 5,705 files |

**`[PI-CHR-010]` The binary now says which source it is, and it is believed.**
The previous build predated `[REQ-VIS-200]` and answered nothing; `--version`
against it started a *second player* instead, because it read the flag as a
database path. The deployed build answers `vaino 0.1.0 (b8bb2e29b94e)`, matching
the development machine exactly.

**`[PI-CHR-015]` It did not match an hour earlier, and the reason was the
cross-build.** The appliance binary is compiled in a Linux container against a
bind-mounted Windows checkout. The worktree there is CRLF and the container's
git has no `autocrlf` to undo it, so `git status --porcelain` reported **104
phantom modifications** and every appliance build stamped `+dirty` however clean
the tree was.

A stamp that always says `+dirty` says nothing — and it was saying it on the one
machine where nobody can check by looking, which is the whole reason
`[REQ-VIS-200]` exists. `build.rs` now asks `git diff --quiet --ignore-cr-at-eol
HEAD`, which agrees with the host. **Found by deploying**: it is not visible from
the development machine at all.

---

## 2. Starting

Taken from the journal, to the millisecond:

| stage | at | cost |
| :--- | ---: | ---: |
| service started | 0.000 s | |
| binary announces itself | 0.014 s | **14 ms** |
| library open, 8,330 passages counted | 11.730 s | **11.7 s** |
| audio device open | 11.888 s | 158 ms |
| web UI listening | 15.292 s | 3.4 s |

**`[PI-CHR-020]` Fifteen seconds to serving, and eleven of them are the
library.** Opening a 1.07 GB SQLite file and counting its radio passages
dominates everything else; the Director's build is the 3.4 s after it. The
deploy script's 90-second health deadline is therefore about six times the
observed need, which is the right side to err on for a machine that is only
reachable through the thing being started.

---

## 3. Resting and playing

Sampled from `/proc` deltas, not `ps %cpu` — that reports the average since the
process began, so it charges the eleven-second library load against every
reading for the rest of the day, and reported 20% for an idle player.

| state | CPU (median) | RSS | temp |
| :--- | ---: | ---: | ---: |
| paused | **1.0%** | 36 MB | 44 °C |
| playing | **4.2%** | 53 MB | 44 °C |
| passage change | 8.6% peak | 53 MB | 44 °C |
| four seeks in ten seconds | 9% median, **37% peak** | 53 MB | 45 °C |

**`[PI-CHR-030]` One core, loafing.** Four per cent of one of four cores to
decode, resample, mix and serve. Twelve threads throughout.

**`[PI-CHR-035]` A seek costs a spike and nothing after it.** 37% for the
moment of a seek — a file open, a decoder seek and a resampler build, which is
exactly what `[REQ-VIS-225]` says it buys — falling back to 4% immediately.
Nothing accumulates: four seeks in ten seconds left the median at 4%.

**`[PI-CHR-040]` Memory is flat.** 52.9 MB to 53.0 MB across five minutes and a
passage change. The machine has 263 MB available with the player running, and
13 MB of swap in use out of 511. Nothing here is close to the edge — which
matters for `[SPEC-BK-060]`, where the open question is whether MPD can be
resident beside it.

---

## 4. Listening

**`[PI-CHR-050]` Zero underruns in five minutes of continuous playback**,
including one passage change. Lock failures: one at startup, none after.

```
  min   cpu%   rss MB   temp  underruns  passage
  0.3    4.2     52.9   45.1          0     4536
  ...
  3.3    8.6     53.0   44.0          0     8730   <- crossfade
  ...
  5.0    4.2     53.0   43.5          0     8730
```

Temperature held between 43.5 °C and 45.1 °C and `get_throttled` stayed
`0x0` — no throttling of any kind, at any point, including during the browse
work below.

---

## 5. The one thing it does badly

**`[PI-CHR-060]` `/browse/albums` takes 25.7 seconds.** Measured over HTTP on
the appliance, for 54 KB of reply:

| endpoint | time | payload |
| :--- | ---: | ---: |
| `/` | 7 ms | |
| `/skins` | 5 ms | |
| `POST /command/skip` | 4 ms | |
| `POST /seek/60000` | 5 ms | |
| `/browse/artists` | 835 ms | 30 KB |
| `/browse/tracks` | 8.2 s | 266 KB |
| **`/browse/albums`** | **25.7 s** | 54 KB |

Thirty times slower than tracks for a fifth of the data, so it is not the
volume. **It is the sort inside the correlated subquery**, measured directly
against the library:

| the same query | time |
| :--- | ---: |
| as the player runs it | **18.05 s** |
| with the inner `ORDER BY` removed | **0.70 s** |

The `ORDER BY` is not removable — without it the answer is wrong, 1,698 albums
instead of 694, because it picks an arbitrary release rather than the chosen
one. The cost is structural: `ALBUM_EXPR` runs once per passage, and
`release_recordings` holds **304,334 rows** for 8,067 recordings — some
thirty-six releases per recording, every reissue and compilation among them —
each needing a join into 61,563 `releases` and a three-column sort.

**`[PI-CHR-065]` A covering index removes nineteen twentieths of it.**

```sql
CREATE INDEX ON release_recordings(mbid, chosen DESC, release_mbid);
```

| | time |
| :--- | ---: |
| without | 18.05 s |
| with | **0.93 s** |
| index build, once | 4.4 s |

Same 694 albums. The index was created, measured and **dropped**: the appliance
is as it was found, because adding an index to a listener's library is a change
to their data and not a measurement of it.

> **This is `[REQ-LIB-165]`'s shape again** — a query that was fine against a
> test library and is not fine against a real one. The development machine
> hides it: the same page there is quick enough that nobody would look.

---

## 6. What was not measured

- **MPD is not installed here**, so nothing in `[SPEC-BK-060]` moved. Every
  MPD measurement in `[SPEC020]` remains a Windows measurement.
- **No Bluetooth speaker was attached** for this run; the output was the
  default sink. Ramp and underrun behaviour over Bluetooth is `[PI004]`'s
  subject and is not re-measured here.
- **Nothing ran for longer than five minutes.** Memory is flat over that
  window; a leak with a slower period would not have shown.

---

**Traceability:** `[PI-CHR-010..065]` · confirms `[REQ-VIS-200]`'s stamp and
`[REQ-VIS-225]`'s cost · bears on `[SPEC-BK-060]` · re-finds `[REQ-LIB-165]`
