# IMPL015: Rehearsing the Rename

**Implementation Guide — three rehearsals run 2026-09-13, all against throwaway copies. Nothing here touched the repository.**

The rename is applied to a disposable tree first, and both checkers are run
against the result. Three rehearsals found **eight** defects, four of them in
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

**`[IMPL-NAM-350]` Clean is "nothing unexplained", not "the audit reports
zero".** Third rehearsal, with a remote set so that surface can resolve: **111 database
paths resolved by evidence, 213 protected, 4,750 substituted, 36 paths renamed,
4 stranded link targets repointed.**

| Result | |
| :--- | :--- |
| `check_docs.py --strict` | **0 errors**, exit 0 |
| `check_rename.py` | **0 surfaces BROKEN**; 12 of 16 surfaces at zero |
| Unexplained occurrences | **0** |

Every surviving occurrence falls in one of two buckets, each by an explicit
decision:

| Count | Bucket | Why it survives |
| ---: | :--- | :--- |
| 213 | Database-name text | Needs a person, see §4 |
| 169 | Guards and naming working papers | They keep the old name on purpose; the papers are deleted entirely at the new-repo seed `[IMPL-NAM-150]`, the guards stay |

A run that cannot produce this table has not finished, whatever the totals say.
**The mechanism is clean; the content is not yet**, and those are different
claims. The rename cannot reach zero until §4 is worked.

---

## 4. The one bucket that is not mechanical

**`[IMPL-NAM-360]` What is left needs reading, because the name means five
different things.** 111 of the 324 were resolved mechanically and safely,
because their *path* says which database they are: `/srv/library/vaino.db` is
the catalogue and becomes `library.db`, `/var/vaino/vaino.db` is the listener
store and becomes `listener.db` `[BOS-RUN-080]`. Those needed no judgement.

The remaining **213 cannot be resolved by pattern at all**, because the bare
name carries at least five distinct meanings:

| Meaning | Example | Correct treatment |
| :--- | :--- | :--- |
| The **pre-split monolith** | `split_database.py vaino.db --library-out` | A file that no longer exists under any name; rewrite as the pre-split database |
| A **real backup on disk** | `` `vaino.db.pre-split-2026…` ``, `` `vaino.db.bak-pre-mulibplay` `` | **Must not change** — these artefacts exist under these names |
| A **live code literal** | `db_path = os.path.join(tmp, "vaino.db")` | Rename with the code |
| A **CLI placeholder** | `usage: import_bundle <vaino.db> <bundle-dir>` | Becomes whichever db the argument actually is |
| A **measurement of the old file** | `` | `vaino.db` | 1.08 GB | `` | Historical; rewrite or retire the row |

The backup row is why no blanket mapping is safe: renaming that text would make
the documentation describe files that do not exist, which is worse than leaving
the old name visible. This bucket is protected through the blanket pass
precisely so a later reader has to choose, one occurrence at a time.
