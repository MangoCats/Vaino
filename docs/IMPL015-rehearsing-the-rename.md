# IMPL015: Rehearsing the Rename

**Implementation Guide — four rehearsals run 2026-09-13, all against throwaway copies. Nothing here touched the repository.**

The rename is applied to a disposable tree first, and both checkers are run
against the result. Four rehearsals found **eight** defects, four of them in
the guards written to catch exactly this class of problem. None would have been
found by reading.

This document is the procedure, what it found, and the accounting that lets a
run be called clean — which is not "the audit reports zero" but "every surviving
occurrence is explained by a decision someone made on purpose."

> **Related:** [IMPL013](IMPL013-executing-the-rename.md) `[IMPL-NAM-070]` — the phases this rehearses · [tools/rename_edit.py](../tools/rename_edit.py) `[IMPL-NAM-077]` — counted edits · [tools/check_rename.py](../tools/check_rename.py) `[IMPL-NAM-020]` — the surface audit

---

## 1. The procedure

**`[IMPL-NAM-300]` Eight steps, on a tree you can throw away.**

1. `git archive HEAD | tar -x -C <tmp>` — a complete tree, no history, nothing
   shared with the working checkout.
2. Copy the **current** `rename_edit.py` and `check_rename.py` in. The archive
   holds the committed versions, which may predate a fix you are relying on
   `[IMPL-NAM-320]`.
3. **Put the driver outside the tree** `[IMPL-NAM-310]`.
4. Protect the database-name text by substituting it to a sentinel, so the
   blanket pass cannot rename it `[IMPL-NAM-120]`.
5. Apply the ordered substitutions, **most specific first** — `vaino` is a
   substring of `vainopi` and of `vainoplayer3`, so a blanket pass run first
   would corrupt both. Each is a counted edit `[IMPL-NAM-077]`.
6. Restore the sentinels. **Protected and restored totals must match exactly**;
   a difference means something outside the intended set was touched.
7. Rename paths, deepest first, then repoint the guards' own directory globs —
   the step `[IMPL-NAM-060]` requires of `check_docs.py` and `check_rename.py`
   alike.
8. Run `check_docs.py --strict` and `check_rename.py`, then account for every
   survivor `[IMPL-NAM-350]`.

Excluded from the blanket pass: both guards, and the four naming working papers
(GUIDE015, IMPL013, IMPL014 and this one) which keep the old name on purpose.
They are listed in `check_rename.py`'s `ALLOW`, and an omission there is
finding 8.

---

## 2. What the rehearsals found

**`[IMPL-NAM-305]` Eight defects, in the order they surfaced.**

| # | Found | Fix |
| :--- | :--- | :--- |
| 1 | The pass **rewrites the guards themselves** — `check_rename.py`'s patterns became `lempi\|vipunen`, inverting it: a successful rename would report as total failure, a failed one as clean | `--exclude` on `rename_edit.py`; the guards are never substituted |
| 2 | An extension-derived file list missed **16 extensionless files** — all fourteen `vaino-*` helper executables, `LICENSE`, `LempiPi/tests/run` — plus `.css`, `.conf`, `.log`, `.bat` | File list comes from the tree, never a guessed extension list |
| 3 | `check_rename.py` had the same blind spot: `bin`, `scripts` and `units` reported **clean** while three helpers held 38 occurrences | Directory-scoped `dir/!doc` globs, keyed to the directory and not the name |
| 4 | A blanket pass produced **189 `lempi.db`**, violating `[IMPL-NAM-120]` | Database-name text is sentinel-protected through the pass |
| 5 | **Excluding a document strands its links.** GUIDE015 correctly kept its prose, but its targets `spec/SPEC007-sampo-architecture.md` and `../BosePi/vaino-bose.service` had moved — `check_docs.py --strict` failed with 2 errors | Link targets in allowlisted documents are repointed even though their prose is not |
| 6 | The driver, living **inside** the tree, had its own substitution literals rewritten mid-run — the table degenerated to `Lempi -> Lempi` | Driver lives outside; `rename_edit.py` caught it as "the replacement may reproduce the pattern" |
| 7 | The driver read an argparse rejection as **"0 matches, nothing to do"** and reported a whole rename as complete | A probe that yields no count is a harness failure, not a zero |
| 8 | `check_rename.py`'s own `ALLOW` list omitted `rename_edit.py` and IMPL015 — two files that keep the old name on purpose — so the audit counted them as residue that could never be cleared | Both added to `ALLOW` |

**`[IMPL-NAM-310]` Finding 5 is the one to remember.** Not renaming a file and
not updating its links are different decisions, and conflating them is silent
until CI runs. A document exempt from the rename is exempt from the *prose*
rewrite only; every path it cites still has to follow the file.

**`[IMPL-NAM-320]` Findings 6 and 7 are the rehearsal's own version of what it
is testing for.** Both were harnesses reporting success while doing nothing —
the same shape as `[IMPL-NAM-077]`'s silent `str.replace`. A rehearsal that
cannot fail loudly is worth no more than the edit it is checking.

---

## 3. What a clean run looks like

**`[IMPL-NAM-350]` The fourth rehearsal is clean: both gates exit 0.**
111 database paths resolved by evidence, 59 CLI placeholders resolved by the
CLI's own second argument, **4,904 substituted**, 36 paths renamed, 13 guard
globs repointed, 4 stranded link targets repointed.

| Gate | Result |
| :--- | :--- |
| `check_docs.py --strict` | **0 errors**, exit 0 |
| `check_rename.py` | **0 occurrences across 16 surfaces, 0 BROKEN** |
| `check_rename.py --expect-zero` on all 16 surfaces | **exit 0** |

What moved it from "nothing unexplained" to actually zero was not a better
mechanism — the mechanism was already right — but withdrawing a rule.
`[IMPL-NAM-120]` had protected every database reference, which held 324
occurrences back permanently on the strength of a concern that applied to four
of them. Applying `[IMPL-NAM-130]` consistently — the project is continuous,
the earlier name was temporary — sends the project-named prose straight through
the pass, where it belonged.

**Three earlier rehearsals were reported as clean and were not.** The first two
reached "nothing unexplained", which is a real property and a different claim;
saying so plainly took being asked a third time. A run is clean when the gates
exit 0, and nothing else counts as the answer.

---

## 4. The one claim about the world

**`[IMPL-NAM-360]` Four lines assert that files exist under names they do not
yet have.** `vaino.db.pre-split-20260907`, `vaino.db.pre-lyrics-import` and
`vaino.db.bak-pre-mulib-art` are real backups on `vainopi`, named in
[IMPL011](../VainoPi/IMPL011-database-split-built.md) and
[PI026](../VainoPi/PI026-startup-preflight.md). The clean run rewrites those four
lines along with everything else, which makes the documentation internally
consistent and factually wrong until the files themselves move.

Phase 3 renames them on the device, inside `[IMPL-NAM-080]`'s existing
copy-verify-remove migration of `/var/vaino` → `/var/lempi`. They are backups of
this project's own database; there is no third party to whom their names mean
anything, and leaving them would strand the only four occurrences the rename
cannot otherwise clear.

This is the single place where a green dry run is not by itself proof: the tree
is correct, and stays correct only if that device step actually happens.
