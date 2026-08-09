"""
Documentation governance checks [GOV-DOC-010..040].

Enforces mechanically what would otherwise depend on discipline. Written after
an audit found that the hazards recorded in docs/inherited/README.md were partly
mis-stated: [INH-HAZ-020] claimed inherited and Vaino identifier tags did not
collide, and measurement found seven exact collisions. Claims about a document
set should be checked by a program, not asserted by a person.

Checks:
  1. Every file under docs/inherited/ carries the MCR- prefix (or is source code).
  2. No Vaino document defines a tag whose prefix is reserved to inherited material.
  3. No exact tag collision between Vaino documents and inherited documents.
  4. Every relative markdown link resolves (code spans excluded).
  5. Vaino documents obey the 100-250 line target [GOV-DOC-010]; warn only.

Usage:
    python tools/check_docs.py            # report
    python tools/check_docs.py --strict   # non-zero exit on any error
"""

import argparse
import glob
import os
import re
import sys

TAG = re.compile(r"\[([A-Z]{2,6}-[A-Z0-9]{2,10}-[0-9]{2,4})\]")
LINK = re.compile(r"\[[^\]\[]*\]\((?!https?:|file:|mailto:|#)([^)]+)\)")
CODESPAN = re.compile(r"`[^`\n]*`")

# Prefixes owned by inherited material. Vaino must not mint new tags with these.
# Vaino MAY cite them (e.g. [MFL-DEF-040]) -- citation is not definition, so the
# collision check below distinguishes the two by looking for a bolded definition.
RESERVED_PREFIXES = {"DBD", "MFL", "MTA", "LD", "AM", "AFS", "XFD", "SSP", "PERF", "ARCH"}

INHERITED_DIR = os.path.join("docs", "inherited")

# Known, accepted tag collisions. Each entry is debt with a stated retirement
# condition -- NOT a way to silence the check. A collision absent from this list
# is an error. Adding an entry requires a reason and a condition for removal.
#
# These seven exist because Vaino's own REQ001 (a v1 artifact on the disposal
# path, [GDE-DIS-010]) reuses tags McRhythm also uses. Renumbering a document
# slated for retirement would be churn; the collision retires with the document.
KNOWN_COLLISIONS = {
    "REQ-QUE-010", "REQ-QUE-020", "REQ-QUE-030", "REQ-QUE-040",
    "REQ-UI-010", "REQ-UI-020", "REQ-UI-030",
}
COLLISION_RETIRES_WHEN = "docs/spec/REQ001-system-requirements.md is replaced [GDE-DIS-010]"


def strip_code(text):
    return CODESPAN.sub("", text)


def vaino_docs():
    out = [p for p in glob.glob("docs/*.md") + glob.glob("docs/spec/*.md")]
    return [p for p in out if INHERITED_DIR not in p]


def inherited_docs():
    return [p for p in glob.glob(os.path.join(INHERITED_DIR, "**", "*.md"), recursive=True)
            if os.path.basename(p) != "README.md"]


def tags_in(path, skip_quoted=False):
    text = open(path, encoding="utf-8").read()
    if skip_quoted:  # drop the import banner, which quotes Vaino tags
        text = "\n".join(l for l in text.split("\n") if not l.startswith(">"))
    return set(TAG.findall(text))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--strict", action="store_true")
    args = ap.parse_args()
    errors, warnings = [], []

    # 1 -- inherited files must be prefixed [INH-HAZ-010]
    for p in glob.glob(os.path.join(INHERITED_DIR, "**", "*"), recursive=True):
        if not os.path.isfile(p) or p.endswith((".cpp", ".h")):
            continue
        base = os.path.basename(p)
        # Vaino-authored files that live here but are not inherited material
        if base in ("README.md", "PROVENANCE.json"):
            continue
        if not base.startswith("MCR-"):
            errors.append(f"[INH-HAZ-010] inherited file lacks MCR- prefix: {p}")

    # 2 -- reserved prefixes must not be defined by Vaino documents
    for p in vaino_docs():
        for t in tags_in(p):
            if t.split("-")[0] in RESERVED_PREFIXES:
                # a definition is bolded at line start; a citation is inline
                for line in open(p, encoding="utf-8"):
                    if f"**`[{t}]`" in line or f"**[{t}]**" in line:
                        errors.append(f"[INH-HAZ-020] {p} DEFINES reserved-prefix tag {t}")
                        break

    # 3 -- exact tag collisions between the two sets
    v = {}
    for p in vaino_docs():
        for t in tags_in(p):
            v.setdefault(t, []).append(p)
    i = {}
    for p in inherited_docs():
        for t in tags_in(p, skip_quoted=True):
            i.setdefault(t, []).append(p)
    for t in sorted(set(v) & set(i)):
        if t.split("-")[0] in RESERVED_PREFIXES:
            continue  # Vaino citing an inherited tag is correct usage
        if t in KNOWN_COLLISIONS:
            warnings.append(f"[INH-HAZ-020] known collision {t} "
                            f"(retires when {COLLISION_RETIRES_WHEN})")
            continue
        errors.append(f"[INH-HAZ-020] tag {t} defined in BOTH "
                      f"{os.path.basename(v[t][0])} and {os.path.basename(i[t][0])}")

    # 4 -- relative links resolve
    for p in glob.glob("docs/**/*.md", recursive=True):
        root = os.path.dirname(p)
        for m in LINK.finditer(strip_code(open(p, encoding="utf-8").read())):
            target = os.path.normpath(os.path.join(root, m.group(1).split("#")[0]))
            if not os.path.exists(target):
                errors.append(f"broken link: {p} -> {m.group(1)}")

    # 5 -- line-count governance, advisory
    for p in vaino_docs():
        n = sum(1 for _ in open(p, encoding="utf-8"))
        if n > 250:
            warnings.append(f"[GOV-DOC-010] {p} is {n} lines (target 100-250)")

    for w in warnings:
        print(f"WARN  {w}")
    for e in errors:
        print(f"ERROR {e}")
    print(f"\n{len(errors)} error(s), {len(warnings)} warning(s)")
    if errors and args.strict:
        sys.exit(1)


if __name__ == "__main__":
    main()
