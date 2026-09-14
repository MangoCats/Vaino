# IMPL015: Rehearsing the Rename

**Implementation Guide — two rehearsals run 2026-09-13, both against throwaway copies. Nothing here touched the repository.**

The rename is applied to a disposable tree first, and both checkers are run
against the result. Two rehearsals found **seven** defects, three of them in the
guards written to catch exactly this class of problem. None would have been
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

Excluded from the blanket pass: both guards, and the three naming working papers
(GUIDE015, IMPL013, IMPL014) which keep the old name on purpose.

---

## 2. What the rehearsals found

**`[IMPL-NAM-305]` Seven defects, in the order they surfaced.**

| # | Found | Fix |
| :--- | :--- | :--- |
| 1 | The pass **rewrites the guards themselves** — `check_rename.py`'s patterns became `lempi\|vipunen`, inverting it: a successful rename would report as total failure, a failed one as clean | `--exclude` on `rename_edit.py`; the guards are never substituted |
| 2 | An extension-derived file list missed **16 extensionless files** — all fourteen `vaino-*` helper executables, `LICENSE`, `LempiPi/tests/run` — plus `.css`, `.conf`, `.log`, `.bat` | File list comes from the tree, never a guessed extension list |
| 3 | `check_rename.py` had the same blind spot: `bin`, `scripts` and `units` reported **clean** while three helpers held 38 occurrences | Directory-scoped `dir/!doc` globs, keyed to the directory and not the name |
| 4 | A blanket pass produced **189 `lempi.db`**, violating `[IMPL-NAM-120]` | Database-name text is sentinel-protected through the pass |
| 5 | **Excluding a document strands its links.** GUIDE015 correctly kept its prose, but its targets `spec/SPEC007-sampo-architecture.md` and `../BosePi/vaino-bose.service` had moved — `check_docs.py --strict` failed with 2 errors | Link targets in allowlisted documents are repointed even though their prose is not |
| 6 | The driver, living **inside** the tree, had its own substitution literals rewritten mid-run — the table degenerated to `Lempi -> Lempi` | Driver lives outside; `rename_edit.py` caught it as "the replacement may reproduce the pattern" |
| 7 | The driver read an argparse rejection as **"0 matches, nothing to do"** and reported a whole rename as complete | A probe that yields no count is a harness failure, not a zero |

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

**`[IMPL-NAM-350]` Clean is "nothing unexplained", not "the audit reports
zero".** The second rehearsal's final state — 324 protected, **4,750
substituted**, 324 restored, 36 paths renamed:

| Result | |
| :--- | :--- |
| `check_docs.py --strict` | **0 errors**, exit 0 |
| `check_rename.py` | no surface BROKEN except `remote`, which a copy with no origin cannot read — an artefact of the rehearsal, not of the rename |
| Unexplained occurrences | **0** |

Every surviving occurrence falls in one of three buckets, each by an explicit
decision:

| Count | Bucket | Why it survives |
| ---: | :--- | :--- |
| 324 | Database-name text | `[IMPL-NAM-120]` — editorial triage, see §4 |
| 129 | The three naming working papers | They keep the old name on purpose; deleted entirely at the new-repo seed `[IMPL-NAM-150]` |
| 38 | The two guards | Excluded so they cannot invert, finding 1 |

A run that cannot produce this table has not finished, whatever the totals say.

---

## 4. The one bucket that is not mechanical

**`[IMPL-NAM-360]` The 324 database-name occurrences need judgement, not
substitution.** They were assumed to be mostly stale pre-split prose. Reading
them showed otherwise: they are overwhelmingly **usage strings and comments**
across 40 Python files, 18 Rust files and the documentation — `dircheck
<vaino.db>`, `usage: flavorcheck <vaino.db> [samples]`, `expected
user@host:/path/to/vaino.db` — where the name is standing in for *"the database
file"*.

That makes all three mechanical options wrong. Renaming them to `lempi.db` names
a file that has not existed since the split. Leaving them keeps the old name in
the new repository. Deleting them removes a usage string a person needs.

The correct treatment is per-occurrence: each becomes whichever of `listener.db`
or `library.db` it actually means, or a generic placeholder where it means
either `[BOS-RUN-080]`. Roughly 300 occurrences of reading, and the reason this
bucket is protected through the blanket pass rather than swept along with it.
