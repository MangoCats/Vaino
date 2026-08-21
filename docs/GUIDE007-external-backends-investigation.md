# GUIDE007: External Backends — an Investigation

**Development Guidance — measured on `investigate/external-players`, not estimated**

What it would cost to let the Director drive MPD or an OpenSubsonic server, whether that makes the project unwieldy for the uses it already serves, and how cleanly it can be kept out of the way of people who do not want it.

> **Related:** [GUIDE006](GUIDE006-director-as-a-guest.md) posed the question · [`player/src/playback.rs`](../player/src/playback.rs) is the spike · [SPEC009](spec/SPEC009-program-director.md)

---

## 1. The seam already existed

**`[GDE-BAK-010]` Measured, not argued.** Three counts, taken from this tree:

| | |
| :--- | ---: |
| Methods `Session` calls on `Engine` | **7** |
| More that a binary's own loop calls | 3 |
| Occurrences of `path` anywhere under `director/` | **0** — a comment and a test's DDL |
| Lines that turn a queue entry into audio | **1** — `PassageDecoder::open(&e.path, …)` |

Every other use of `entry.path` is an error message, a filename for display, or `relink`, which is a different subsystem.

**`[GDE-BAK-015]` `Engine` satisfies a `Playback` trait with no change to `engine.rs` at all.** The spike defines the trait, implements it for `Engine` by forwarding to methods that already existed with matching signatures, and **237 tests pass**. Nothing was refactored to make that true; the separation of selection from playback was already real and merely unnamed.

**So the cost of the seam itself is approximately zero.** It has been paid, by whoever kept the Director from reaching for a file path.

---

## 2. What it would actually cost

**`[GDE-BAK-020]`** The trait is not the work. The work is one adapter per protocol, and the mapping problem behind both.

| Piece | Estimate | Notes |
| :--- | :--- | :--- |
| `Playback` trait + `Capabilities` | **done, ~130 lines** | in the spike, tested |
| MPD adapter | few hundred lines | line protocol over TCP; **no new dependency**, `std::net` suffices |
| OpenSubsonic adapter | more | HTTP + JSON; wants an HTTP client, and that is the dependency question below |
| **Passage → server song mapping** | **the real unknown** | see `[GDE-BAK-025]` |
| Sampo | **nothing** | it writes `vaino.db` and does not care who plays |

**`[GDE-BAK-025]` The mapping is the part with no obvious answer.** Vaino selects a passage in *its* database; MPD names a file by URI relative to *its* music root; OpenSubsonic names a song by an opaque server id. Nothing guarantees the two libraries even contain the same files. Candidate keys — `recording_mbid` where the server exposes it, path suffix, `(artist, title, duration)` — each fail differently, and this is the same class of problem `[SPEC-RLK-025]` already made expensive once. **It should be prototyped before anything else**, because a clean trait over an unreliable mapping is a well-organised way to play the wrong song.

---

## 3. Would it hurt the uses already served?

**`[GDE-BAK-030]` The fidelity question is much bigger than DAO captures, and the measurement says so.** Taken from the live library, 8,330 radio passages:

| | |
| :--- | ---: |
| passages that are the whole file | **116 — 1.4%** |
| passages that are genuine slices | **8,214 — 98.6%** |
| files holding more than one radio passage (DAO) | 191 of 5,709 |

The trim is not a DAO phenomenon. It is on nearly every track, because the Album/Radio duality `[GDE-BMK-030]` is what radio passages *are*. How much it removes, over the 5,518 single-passage files:

| | omitted |
| :--- | ---: |
| median | **6.7 s** |
| p75 | 14.5 s |
| p90 | **61.3 s** |
| p95 | 106.1 s |
| max | 1,976 s |

Only 12.5% of files are within a second of their whole length. **A whole-file backend would put a median of nearly seven seconds of dead air into most tracks**, a minute or more into one in ten. That is not a corner case; it is the listening experience.

**`[GDE-BAK-035]` MPD escapes this and OpenSubsonic does not.** `rangeid {ID} {START:END}` has specified the portion of a song to play since **MPD 0.19**, in fractional seconds. That is Vaino's passage model almost exactly.

| | spans | gain | ramps |
| :--- | :---: | :---: | :---: |
| built-in engine | ✓ | ✓ | ✓ |
| **MPD** | **✓** | ✗ | ✗ |
| OpenSubsonic | ✗ | ✗ | ✗ |

So the two targets are **not comparable**, and GUIDE006 was wrong to rank OpenSubsonic first on reach alone. MPD preserves what the library is built around; OpenSubsonic reaches more clients while discarding it. Reach is worth less than it looked.

**`[GDE-BAK-040]` The capability declaration is what keeps this honest.** `Capabilities::would_misplay` answers, per passage, whether a backend would play something other than what was chosen — and a whole-file backend is *fine* for a passage that covers its file. This is `[PI3-API-030]`'s rule applied to a network: an output that accepts everything and plays the wrong thing is a player lying about what it is doing. A backend that cannot clip should **refuse the passage and say so**, not play forty minutes and hope.

---

## 4. Keeping it out of the way

**`[GDE-BAK-050]` Cargo features, default off, and the appliance carries nothing.** Nothing in this repository is feature-gated today, and this is the case that earns it:

```toml
[features]
default = []
mpd = []                    # std::net only
subsonic = ["dep:ureq"]     # or whatever HTTP client
```

A build without them contains no adapter code, no adapter dependencies, and no larger binary — which matters, because the aarch64 player is **6.8 MB** on a 512 MB appliance where *"every dependency is a memory decision"* `[REQ-HW-140]`.

**`[GDE-BAK-055]` Three further rules keep the blast radius at zero:**

1. **No schema change.** Any passage-to-server mapping lives in a **sidecar**, not `vaino.db` — the precedent already exists twice, in `vaino_new.idchecks.db` and `<library>.console.db` `[IMPL-SUI-055]`. A local-only user never grows a table they will not fill `[SPEC-SC-015]`.
2. **The trait is internal.** `Engine` implements it and behaves identically; no local code path changes, and the spike proves it by leaving `engine.rs` untouched.
3. **Static dispatch.** Make `Session` generic over `P: Playback` rather than holding a `dyn`, and the local build monomorphises to what it compiles today.

**`[GDE-BAK-060]` What genuinely does get harder.** Honesty about the cost, not just the containment:

- **Two more ways to be wrong in the same report.** Every diagnostic that says "playing" now has a backend behind it, and the appliance's dummy-sink episode is the standing reminder of how that goes wrong `[PI3-FOUND-050]`.
- **The test matrix widens.** A protocol adapter wants a fake server, or it is untested.
- **`take_dropped` becomes ambiguous.** Locally it means a file would not open. Remotely it may mean the server forgot the song, the network went away, or someone else emptied the queue — and rotation bookkeeping `[REQ-PD-112]` treats those identically today.

---

## 5. Recommendation

**`[GDE-BAK-070]` If any of it is built, build MPD, and build the mapping first.** MPD keeps the Album/Radio duality, needs no new dependency, and is the smaller adapter. OpenSubsonic buys client reach at the price of the thing 98.6% of this library is shaped by — which is a poor trade for *this* library, whatever it is for someone else's.

**`[GDE-BAK-075]` And nothing needs deciding to keep the option.** The seam is already there and costs nothing to leave named: the trait compiles, `Engine` satisfies it, tests pass. That is the whole of what this branch establishes, and it can sit unused indefinitely without weighing anything down.

---

## 6. Open

1. **`[GDE-BAK-080]` The mapping, prototyped against a real Navidrome or MPD instance.** Nothing else should start first `[GDE-BAK-025]`.
2. **`[GDE-BAK-085]` Whether gain can be carried to MPD at all.** ReplayGain is per-file; Vaino's `gain_db` is per-passage `[SPEC-SC-040]`, and two passages in one file may legitimately differ.
3. **`[GDE-BAK-090]` Whether a remote backend can report plays precisely enough for rotation.** Stage A is the part with six years of tuning behind it `[GDE-PD-010]`, and it is worth less if the history it reads is approximate.

---

**Traceability:** `[GDE-BAK-010..090]` · derived from `[GDE-EXT-020]`, `[GDE-BMK-030]`, `[REQ-HW-140]`, `[PI3-API-030]`
