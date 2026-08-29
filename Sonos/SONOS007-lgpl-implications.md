# SONOS007: The LGPL Obligations of Statically Linking LAME, in Full

**Development Guidance — investigated on `Sonos`, 2026-08-28**

[SONOS006](SONOS006-lame-linking.md) `[GDE-SONOS-590]` called static linking's LGPL fit "plausible," deliberately short of a legal conclusion. This reads the actual license text — LGPLv3 §0 and §4, and GPLv3 §6 by reference — against what Vaino specifically would do, rather than reasoning from the license's reputation.

> **Related:** [SONOS006](SONOS006-lame-linking.md) `[GDE-SONOS-520..610]` · quoted text: [gnu.org/licenses/lgpl-3.0](https://www.gnu.org/licenses/lgpl-3.0.html), [gnu.org/licenses/gpl-3.0](https://www.gnu.org/licenses/gpl-3.0.html)

---

## 1. Two licenses are actually in play, not one

**`[GDE-SONOS-620]` LAME's own C source is LGPL version 2 (the "GNU Library General Public License"); the Rust binding crate is separately declared LGPL-3.0.** `mp3lame-sys` vendors LAME's original source under its own upstream `COPYING`, which is the 1991-era LGPLv2 text — the same license family `shine` carries `[GDE-SONOS-500]`. The Rust glue code around it (`mp3lame-sys`/`mp3lame-encoder` themselves) is separately licensed LGPL-3.0 by its author, per crates.io. LGPLv2 code conventionally carries FSF's standard "version 2, or (at your option) any later version" clause, which would let it be treated under LGPLv3 terms instead — this project did not verify that clause against LAME's own per-file headers, so the conservative reading is to comply with **both**: satisfy LGPLv3 §4 for the whole combined result, which is at least as strict as LGPLv2's own §6 on every point that matters here.

---

## 2. The one fact that resolves most of the worry: Vaino's own code does not relicense

**`[GDE-SONOS-630]` This is the entire reason "Lesser" exists.** LGPLv3 §0's own definitions: **"the Library"** is LAME — "a covered work governed by this License, other than an Application or a Combined Work." **"An Application"** is "any work that makes use of an interface provided by the Library, but which is not otherwise based on the Library" — this is Vaino, exactly: it calls LAME's public encoding API and is not a derivative of LAME's own code. **"A Combined Work"** is what results from linking the two — the compiled `vaino-player` binary, once LAME's object code is inside it. **The License applies to the Library and to the Combined Work's own obligations under §4 — never to the Application's own source.** Vaino's MIT license, on every line of code outside LAME's own, is untouched by this regardless of static or dynamic linking.

---

## 3. What §4 actually requires of the Combined Work, verbatim, checked one at a time

**`[GDE-SONOS-640]` §4(a) — prominent notice that the Library is used, and covered by this License.** Not yet done; a small, concrete gap. Vaino's own `--version` output and web UI already say what build this is `[REQ-VIS-200]` — the natural place to add one line once this ships, the same reasoning that already puts the build id somewhere a person actually looks rather than only in a file nobody opens.

**`[GDE-SONOS-650]` §4(b) — accompany the Combined Work with a copy of the GNU GPL and this license document.** Not yet done; needs a `THIRD-PARTY-LICENSES` file (or equivalent) carrying LAME's own `COPYING` text (LGPLv2) and the LGPLv3 text, alongside whatever the repository already does for its own `LICENSE` file. A single new file, not a process change.

**`[GDE-SONOS-660]` §4(c) — if the Combined Work displays copyright notices during execution, include the Library's among them.** Only binding if Vaino's own UI shows copyright notices at all today — worth checking before assuming it applies, and easy to satisfy alongside `[GDE-SONOS-640]` if it does.

**`[GDE-SONOS-670]` §4(d) — one of two options, and this project's own openness already leans toward the one that fits.**

- **Option 1**, verbatim: *"Use a suitable shared library mechanism for linking with the Library."* This is the dynamic-linking path `[SONOS006]` already declined for unrelated (maintainability) reasons — moot here, not because it wouldn't satisfy the license (it would, trivially) but because static linking was the actual choice.
- **Option 0**, verbatim: *"Convey the Minimal Corresponding Source under the terms of this License, and the Corresponding Application Code in a form suitable for, and under terms that permit, the user to recombine or relink the Application with a modified version of the Linked Version to produce a modified Combined Work."* **This is very plausibly already satisfied by how this project already operates**, not by anything new: `build/Dockerfile.aarch64`, `player/Cargo.toml`, and the full source of everything Vaino compiles are already public. A recipient can already substitute a modified LAME source tree into the same build and produce their own modified Combined Work by rebuilding — which is what "recombine or relink" asks for, satisfied by rebuildability rather than by literal object-file swapping. **Read carefully, not asserted as certain**: this is a defensible, good-faith reading of the text against this project's actual practice, not a substitute for real legal review if it ever matters commercially.

**`[GDE-SONOS-680]` §4(e) — Installation Information, and this is the clause worth understanding precisely for a home appliance, not skipping past.** §4(e) only applies "to the extent... otherwise required... under section 6 of the GNU GPL" — which in turn only bites when **both** of two things are true: the work is conveyed "in, or with, or specifically for use in, a **User Product**," *and* that conveying is "part of a transaction in which the right of possession and use of the User Product is transferred to the recipient in perpetuity or for a fixed term."

**`[GDE-SONOS-690]` vainopi plausibly *is* a "User Product" by GPLv3's own definition — worth naming honestly, not argued away.** GPLv3 §6: a User Product is "any tangible personal property which is normally used for personal, family, or household purposes" — a home music appliance fits that description about as squarely as a definition gets. **But the obligation still only triggers on a qualifying transaction — this project handing a built appliance to someone else, not building one for yourself.** As this project stands today (documentation and source published; the household's own vainopi built and run by the same people who wrote the code), that transaction has not happened.

**`[GDE-SONOS-700]` And if it ever does, the obligation is already met by what `VainoPi/` already documents.** "Installation Information" is defined as whatever is needed "to install and execute modified versions" of the software "from a modified version of its Corresponding Source" — vainopi has no secure boot, no signing, no vendor lock of any kind; it is an ordinary Raspberry Pi a recipient can already reflash and redeploy to entirely by following `VainoPi/HOWTO.md` and `deploy-player.sh` as they already exist. There is no lockdown this clause would need to unwind. If this project ever does convey a built appliance to someone else, the honest answer is "the instructions already published are the Installation Information" — not a new deliverable.

---

## 4. What to actually do, concretely

**`[GDE-SONOS-710]` Two small, low-cost additions close the open gaps; nothing else changes.**

1. Add a `THIRD-PARTY-LICENSES` file (or similar) carrying LAME's `COPYING` (LGPLv2) and the LGPLv3 text `[GDE-SONOS-650]`.
2. Add one line to wherever Vaino already states its own build identity — `--version` output, the web UI's about text — naming LAME and that it is LGPL-licensed `[GDE-SONOS-640]`, `[GDE-SONOS-660]`.

**`[GDE-SONOS-720]` Nothing about Vaino's own license, its build process, or its deployment tooling needs to change.** The Application/Combined-Work split `[GDE-SONOS-630]` is precisely why; the Minimal-Corresponding-Source condition `[GDE-SONOS-670]` is very plausibly already met by this project's existing openness; the Installation Information condition `[GDE-SONOS-700]` is already met by documentation that already exists, for a transaction that has not yet occurred.

---

## 5. The honest limit of this document

**`[GDE-SONOS-730]` This is a careful reading of the license text against this project's actual practice — not a legal clearance.** It is written with the same discipline this whole investigation has tried to apply elsewhere: quote the actual source rather than its reputation, and say plainly where a reasonable reading stops short of certainty `[GDE-SONOS-670]`, `[GDE-SONOS-590]`. If this project ever moves from "a household runs its own build" to actually conveying built appliances to other people, that is the moment a real legal review earns its cost — not before, and not as a substitute for one now.

---

**Traceability:** `[GDE-SONOS-620..730]` · derived from `[GDE-SONOS-520..610]`
