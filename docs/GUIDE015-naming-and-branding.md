# GUIDE015: What This Project Should Be Called

**Development Guidance — decided 2026-09-13, after a trademarked product with the same name, in the same category, on the same hardware was found in the field**

The project renames from **Vaino** to **Lempi**. This document records the
conflict that forced the change, the screen the replacement had to survive, the
candidates the screen killed and why, and what the rename costs. It is written
at length because two of the finalists were eliminated by facts a casual search
would not have surfaced, and the next person naming something here should not
have to rediscover the method.

> **Related:** [GOV001](GOV001-document-hygiene.md) `[GOV-DOC-040]` — the check a rename must keep green · [GOV002](GOV002-sources-of-truth.md) `[GOV-SRC-010]` — ranking two answers to one question, which is the whole of §3 · [GUIDE011](GUIDE011-deploy-script-naming.md) `[GDE-DEP-010]` — the other document here about a name proving load-bearing · [SPEC007](spec/SPEC007-sampo-architecture.md) `[SPEC-SA-010]` — Sampo's identity, which does **not** change

---

## 1. Why the name has to change

**`[GDE-NAM-010]` There is a commercial, trademarked Väinö, and it is this
product.** SuppoNexus Technologies (Peoria, Arizona) sells **Väinö** at
`supponexus.com/products/vaino`: a $49.95 one-time-purchase music server for
Raspberry Pi. It indexes a library from NAS or USB, streams to an attached DAC
or headphones, is controlled from a web browser or iOS/Android companion apps,
supports multi-room through satellite Pis, and markets itself on "your music
never leaves your network."

Same name. Same Kalevala derivation. Same product category. Same hardware. Same
privacy pitch. The mark is theirs and so is the `.com`. There is no
different-field argument available here, and no amount of having-been-here-first
that survives contact with a registered mark — we were not first in any sense a
register recognises.

**`[GDE-NAM-020]` Sampo moves too, and becomes Vipunen. Corrected 2026-09-13.**
An earlier revision of this section said Sampo's neighbours "sit in other
categories and do not touch a Python tool that never leaves the workbench," and
kept the name. Both halves were wrong, and the method that produced them is the
error worth recording: Sampo was screened with a web search rather than the
register — the precise mistake `[GDE-NAM-050]` exists to prevent, applied to the
player's name but not the builder's.

The register says **91 exact SAMPO marks, 52 live, 16 of them in classes 9, 41
or 42**:

| Office | Classes | Owner |
| :--- | :--- | :--- |
| **FR** | **9, 15, 41** | Alexander Mihalic (2015) — the Sampo instrumental-augmentation device |
| CN ×4, IN, IT, KR | 9 (IT also 11) | **Sampo Corporation** — Taiwanese consumer electronics, from 1982 |
| FI | 9, 41 | Fennica Gaming Oy |
| FI | 6, 7, 8, **9**, 11, 17, 21, 28 | Metso Outotec Oyj, filed **1936** |

The French mark is the disqualifying one: **class 15 is musical instruments**,
held alongside software and entertainment by the maker of a device that
processes musical audio in software. That is Väinö's shape exactly.

The second half was also wrong on our own evidence: `[SPEC-SA-010]` declares
Sampo **a separate project with its own repository, licence and platform
envelope** — built to be distributed, not a workbench tool.

**`[GDE-NAM-025]` Vipunen is the replacement, and it fits better than Sampo
did.** Antero Vipunen is the ancient giant who had swallowed all the world's
songs; Väinämöinen descended into his belly to take the words he lacked. That is
what the builder does — extract knowledge from raw material and hand it over —
where "a mill that grinds abundance" was always a loose metaphor for feature
extraction. `[SPEC-SA-010]`'s own phrase, *a separate artifact the bard depends
on but never contains*, becomes literally true of a separate being he must enter.

It is also the cleanest name in this entire investigation: **one exact mark
worldwide, not live**, zero in any relevant class, and crates.io, PyPI, npm,
`vipunen.app`, `.fm`, `.io` and `.dev` all free. The one namesake is
`vipunen.fi`, the Finnish education ministry's statistics portal — a government
service, no mark, no category overlap. **Sammas**, the Estonian cognate of
Sampo, is the fallback if the milling metaphor is worth keeping: 8 live marks,
none in classes 9, 41 or 42.

---

## 2. What the rename costs

**`[GDE-NAM-030]` 3,360 occurrences across 387 of 512 tracked files — and zero
governance tags.** Measured 2026-09-13:

| | |
| :--- | :--- |
| Occurrences of `vaino`, any case, tracked files | 3,360 |
| Files containing at least one | 387 of 512 |
| Governance tags containing `VAINO` | **0** |
| Paths carrying the name | `VainoPi/`, [BosePi/vaino-bose.service](../BosePi/vaino-bose.service) and its siblings, `vaino.db`, the `MangoCats/Vaino` remote |

The zero is the important number. The identifier graph `[GOV-DOC-040]` checks is
untouched by a rename, so no tag is redefined, orphaned or renumbered. What the
rename *does* touch is cited **paths**, and those are checked too — which is why
§6 insists the directory renames and their citations land in one commit.

---

## 3. How the candidates were screened

**`[GDE-NAM-040]` Five gates, cheapest disqualifier first.** Roughly forty names
entered; one left. In order:

1. **Meaning, in every language likely to read it.** Cheapest, and the most brutal.
2. **Package registries** — crates.io, PyPI, npm.
3. **GitHub** — exact repository and account names, distinguished from substring noise.
4. **Domains** — `.com/.io/.app/.fm/.dev/.audio` and the country code.
5. **The global trademark register** — TMview, which federates 70+ offices including USPTO, EUIPO and WIPO.

**`[GDE-NAM-050]` Gate 5 outranks gates 2–4, and the difference is not
academic.** This is `[GOV-SRC-010]`'s discipline applied to a naming question:
when two sources answer *is this name free?*, rank them by what they actually
measure. A clean crates.io and a free `.app` measure whether a developer has
claimed the string. They say nothing about whether a company can stop you using
it. **Vaski passed gates 1–4 cleanly and failed gate 5 outright** — see
`[GDE-NAM-070]`. Any future name here gets gate 5 run before it is announced,
not after.

A caveat that belongs in the record rather than in a footnote: TMview is a
screening tool, not a clearance search, and nobody involved is a lawyer. It is
sufficient to rule names *out*. If Lempi ever ships as a paid product against
Väinö's $49.95, a professional clearance search on the final name is owed.

---

## 4. What the screen rejected

**`[GDE-NAM-060]` The register, not taste, did most of the work.** "Live cl.
9/41/42" counts live marks in the classes a music player actually occupies:
software and recorded media (9), entertainment (41), software development
services (42).

| Candidate | Meaning | Killed by |
| :--- | :--- | :--- |
| Kanteletar | Lönnrot's 1840 song collection | *nothing — survives, see `[GDE-NAM-100]`* |
| Kante / Cante | (proposed short forms of the above) | Not a valid Finnish stem; N'Golo Kanté owns the search space; German *Kante* = "edge"; the English ear hears *cant* |
| **Kulta** | gold; darling | **कुलटा (kulaṭā)** — Hindi/Sanskrit for an unchaste woman, derogatory, same pronunciation |
| **Vaski** | copper; brass | Live EUTM + US + 22 Chinese registrations — see `[GDE-NAM-070]` |
| Kiuru | skylark | Methics Oy, live FI mark, cl. 9 + 42 |
| Ilmatar | air-maiden, mother of the bard | Ilmatar Energy HoldCo, live **EUTM** including cl. 9 |
| Tuuli | wind | **Nokia** holds cl. 9 in EU, WIPO, CA, BR, AR, TH |
| Sointu | chord, harmony | Sointu Group Oy, GB + EUTM cl. 9; FI cl. 9+41 and 9+42 |
| Vilja | grain — what the Sampo grinds | Vilja Solutions AB, cl. 9+42 in SE, EU, GB, WIPO |
| Kuura | frost | Nordtech Trading Oy, EUTM **and** US, cl. 9+42 |
| Hilla | cloudberry | `vaadin/hilla` — a live, well-known web framework |
| Ruska | autumn colour | Reads as "Russian woman" in Czech, Slovak and Polish |
| Otso | the bear | 26 live exact marks; most of the cl. 36 ones held by **Sampo Oyj** |
| Aamu, Kajo, Rusko | dawn, gleam, dawn-glow | All carry live cl. 9 or cl. 42 marks |
| Bygul, Palug | Freyja's cat; Cath Palug | Register-clean, but neither is pronounceable or pleasant in English |

**`[GDE-NAM-070]` Vaski is the one to remember, because it looked clean.**
crates.io, PyPI and npm were free; `vaski.app` and `vaski.fm` were free; GitHub
held nothing but surname noise. The register said otherwise:

- **EUIPO 019083044 — VASKI, classes 7, 37, 40, 41, 42, live to 2034**, owned by
  **Vaski Group Oy**, a Finnish industrial-automation group that renamed *itself*
  to Vaski in 2021, turns over €25M+, and is presently launching Vaski USA and
  Vaski Mexico. Class 42 is software design and development.
- **US "V VASKI"**, same owner, same class spread.
- **22 live Chinese registrations** of VASKI across classes 2–45, by an unrelated
  holder, Jiangsu Tiankong Holding Group.
- **US 4509107 — VASKI, classes 9 and 41, Alex Brouwer**, now lapsed. That is
  Vaski the dubstep producer, releasing under the name since 2008 — exactly the
  pattern that leaves surviving common-law rights in the music space.
- `vaski.fi` belongs to **Turun kaupunki**, the City of Turku: its public
  **library** network.

Adopting that in order to escape a name collision would have been a lateral
move, not an escape.

---

## 5. The decision

**`[GDE-NAM-080]` Lempi.** Poetic-archaic Finnish for love; from Proto-Finnic
`*lempi`, probably first meaning *heat* or *burning*, cognate with `lämmin`
("warm") and Estonian `lemme` ("spark"). Five letters, two syllables, spelled as
heard, ends on a bright vowel. Three things earn it the name rather than merely
clearing it:

1. **In compounds, `lempi-` is the Finnish prefix for "favourite."**
   *Lempilaulu* — favourite song. *Lempimusiikki* — favourite music.
   *Lempilaulaja* — favourite singer. For a player whose whole job is learning
   which passages this listener loves and putting them in the stream, that is
   the most exact word available in the language — and it is wholly opaque in
   English, where it reads as a soft two-syllable name. This is the property
   Vaino had, and its absence is why the merely register-safe alternatives felt
   arbitrary.
2. **It is the root of `lemmikki`, the ordinary Finnish word for a pet animal.**
   The MangoCats tie is in the word's own family tree rather than bolted on.
3. **The Kalevala line survives.** Lemminkäinen — the reckless singer-hero of
   Sibelius's four legends — is *son of Lempi*. Nothing already written about why
   this project carries a Finnish song-tradition name has to be rewritten.

**`[GDE-NAM-090]` The evidence, in full.** Screened 2026-09-13.

| Gate | Result |
| :--- | :--- |
| Trademark (TMview, 70+ offices) | 18 exact marks worldwide, **6 live, none in class 9, 41 or 42** |
| — the six live marks | FI cl. 11+19 (2025); FI cl. 44 health services; FI cl. 14 jewellery; FI cl. 32+33 Teerenpeli brewery (1999); FI cl. 25 clothing; ES cl. 18 leather |
| crates.io / PyPI / npm | all free |
| Domains available | `lempi.io`, `lempi.fm`, `lempi.co`, `lempi.audio`, `lempi.sh` |
| Domains taken | `.com`, `.net`, `.org`, `.fi`, `.dev`, `.app`, `.ai` |
| GitHub | account `lempi` taken; 148 name matches, all substring noise (`lempify`, `Lempira`, `twitter-lempire`); no exact project |

**`[GDE-NAM-100]` What Lempi does not carry, stated rather than glossed.**

- **No colour.** The brief wanted a name that would also suit a mango-coloured
  cat. No clean, lyrical colour word exists — Kulta, Hilla, Ruska, Vaski, Aamu,
  Kajo and Rusko were all killed above, and between them they exhaust the axis.
  Colour lives in **MangoCats**, the house brand; the product name carries the
  song. Lempi's `*lempi` = "heat, spark" etymology is the nearest thing to a
  colour that any surviving candidate offers.
- **One euphemistic sense.** Among its poetic meanings, `lempi` can stand for
  lovemaking. Mild — roughly where English "amour" sits — but a Finnish reader
  hears the register, and it should not come as a surprise later.
- **It is a common word and a common given name**, so it is weak as a mark and
  noisy in search (430 substring hits). A deliberate trade: an unregistrable
  common word that cannot be lost beats a distinctive one somebody else already
  owns.
- **Residual, unchased:** Wiktionary lists `lempi` as also occurring in six other
  languages. Those entries were not individually checked.
- **Kanteletar remains the fallback** — one exact mark worldwide, none live,
  every registry and domain free. It is the safest name the screen found, and it
  was rejected only on length.

---

## 6. Carrying out the rename

> **The executable plan is [IMPL013](IMPL013-executing-the-rename.md).** The
> sketch below is the shape; IMPL013 is what gets run. A review on 2026-09-13
> found this section covers the documentation surface and misses five others —
> the compiled artifact, 23 environment variables, thirteen installed helpers,
> live appliance data, and the hostnames. Two of those fail silently: see
> the env-var trap `[IMPL-NAM-040]` and the governance gap `[IMPL-NAM-060]`.
> Read IMPL013 before touching anything.

**`[GDE-NAM-110]` The directory renames and their citations must land in one
commit.** `[GOV-DOC-040]` checks that every doc-cited path exists in the tree. A
commit that renames `VainoPi/` without rewriting the documents citing it, or the
reverse, fails `--strict` in CI. Because `[GDE-NAM-030]` measured zero affected
tags, the atomic unit is exactly: path renames, plus every citation of those
paths.

Suggested order — each step its own commit, each independently green:

1. Prose and identifiers in `docs/`, `README.md`, `HOWTO.md`. No paths move.
2. Rust and Python sources, under `player/` and `tools/`.
3. `VainoPi/` → `LempiPi/`, plus every citation of a path inside it. **One commit.**
4. Service units, `vaino.db` → `lempi.db`, deploy scripts under `build/`.
5. The GitHub remote, last, once nothing else references the old one.

Per `[GDE-DEP-010]`'s lesson, state the assumption in each step's commit message
rather than letting a silent one pass: a rename that half-happened reads exactly
like one that fully happened.

**`[GDE-NAM-120]` Deliberately left open.**

- **Hostnames.** `vainopi` as a machine name is separate from the project name,
  and changing it reaches beyond this tree into SSH configs and deploy targets.
  Not part of this rename.
- **No short form.** `lempi` is already five letters; nothing needs abbreviating.
  The earlier hunt for a short form of a longer name is precisely what produced
  the Kante/Cante dead end.
- **Sampo** stays, on `[GDE-NAM-020]`'s reasoning. Revisit only if it is ever
  distributed as a product.
