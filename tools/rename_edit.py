#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Counted mechanical edit `[IMPL-NAM-077]`: replace, but only the number you said.

Written after a scripted edit during the rename planning silently matched
nothing. An earlier edit had re-wrapped the line being searched for, Python's
`str.replace` returned the string unchanged, the command exited 0, and the
change simply did not happen. Nothing reported anything. That is the same
failure the rename plan is largely about -- a change that reports success while
doing nothing -- and in a 4,500-occurrence rename executed by scripted edits it
is not a hypothetical.

`sed -i`, `perl -pi -e` and `str.replace` all share the defect: **a pattern that
matches nothing is not an error to them.** This tool makes the expected count
part of the command, so a no-op fails instead of passing quietly.

Why an exact count rather than "at least one": a partial match is the common
case, not the rare one. A pattern that should hit thirty lines and hits three --
because the rest wrapped differently, or use a different case, or a variant
spelling -- is a half-finished rename, and "greater than zero" reports it as a
success.

Dry run is the default. Several agents work in this tree; a tool that rewrites
files should not do so because an argument was forgotten.

Usage:
    python tools/rename_edit.py --pattern 'vaino_player' --replace 'lempi_player' \\
        --expect 101 'player/src/**/*.rs'
    ... --write        # apply, then verify none remain

Exit codes: 0 ok, 1 count mismatch or unreadable file, 2 bad invocation.
"""

import argparse
import glob
import os
import re
import sys


def collect(patterns):
    out = []
    for pat in patterns:
        out.extend(glob.glob(pat, recursive=True))
    return sorted({p.replace("\\", "/") for p in out if os.path.isfile(p)})


def read(path):
    """Read preserving exact line endings. newline='' matters: translating
    LF to CRLF on a Windows checkout would corrupt the shell scripts that
    [IMPL-NAM-045]'s .gitattributes rule exists to keep as LF."""
    with open(path, encoding="utf-8", newline="") as fh:
        return fh.read()


def write(path, text):
    with open(path, "w", encoding="utf-8", newline="") as fh:
        fh.write(text)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--pattern", required=True, help="regex to replace")
    ap.add_argument("--replace", required=True, help="replacement text")
    ap.add_argument("--expect", type=int, required=True,
                    help="exact number of matches expected across all files")
    ap.add_argument("--ignore-case", action="store_true",
                    help="off by default: a case-insensitive rename pattern is "
                         "how the audit once double-counted a whole surface")
    ap.add_argument("--exclude", action="append", default=[], metavar="SUBSTR",
                    help="skip paths containing this substring; repeatable. "
                         "The guards must be excluded: a blanket pass rewrites "
                         "check_rename.py's own patterns to hunt the new name "
                         "and inverts it [IMPL-NAM-079]")
    ap.add_argument("--write", action="store_true",
                    help="apply the edit; without it this only reports")
    ap.add_argument("paths", nargs="+", help="file globs")
    args = ap.parse_args()

    files = [f for f in collect(args.paths)
             if not any(x in f for x in args.exclude)]
    if not files:
        print(f"BROKEN  no files matched {args.paths} -- refusing to report a "
              f"count of zero for a file set that does not exist")
        return 1

    rx = re.compile(args.pattern, re.IGNORECASE if args.ignore_case else 0)

    total, per_file, unreadable = 0, [], []
    for path in files:
        try:
            text = read(path)
        except (OSError, UnicodeDecodeError) as exc:
            unreadable.append((path, exc))
            continue
        n = len(rx.findall(text))
        if n:
            per_file.append((path, n))
            total += n

    if unreadable:
        for path, exc in unreadable:
            print(f"ERROR   cannot read {path}: {exc}")
        print(f"FAIL    {len(unreadable)} unreadable file(s) -- a file that "
              f"cannot be scanned is not a file without matches")
        return 1

    for path, n in per_file:
        print(f"{n:6}  {path}")
    print(f"{total:6}  TOTAL across {len(files)} file(s) scanned, "
          f"{len(per_file)} with matches")

    if total != args.expect:
        print(f"FAIL    expected {args.expect}, found {total}. Nothing written.")
        if total == 0:
            print("        A pattern matching nothing is the failure this tool "
                  "exists to catch -- check wrapping, case and spelling before "
                  "adjusting --expect.")
        return 1

    if not args.write:
        print(f"OK      {total} match(es) as expected. Dry run; pass --write to apply.")
        return 0

    changed = 0
    for path, _n in per_file:
        text = read(path)
        new, n = rx.subn(args.replace, text)
        if n:
            write(path, new)
            changed += n

    if changed != args.expect:
        print(f"FAIL    replaced {changed} but expected {args.expect}; the tree "
              f"is now in an unknown state -- inspect `git diff` before continuing")
        return 1

    remaining = 0
    for path, _n in per_file:
        try:
            remaining += len(rx.findall(read(path)))
        except (OSError, UnicodeDecodeError):
            print(f"ERROR   cannot re-read {path} to verify")
            return 1
    if remaining:
        print(f"FAIL    {remaining} match(es) of the original pattern survive "
              f"the write -- the replacement may reproduce the pattern")
        return 1

    print(f"OK      replaced {changed}, none remain.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
