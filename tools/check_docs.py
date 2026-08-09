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
  6. Every cited tag is defined somewhere; no tag is defined twice.

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

# Peel leading markdown one token at a time. '*' counts as a bullet only when
# followed by whitespace, so '**' (bold) survives to mark a definition.
# Emoji/symbol range is deliberately broad (U+2190-U+2BFF plus the emoji planes):
# a narrow list silently broke definition detection when a document used a
# character that had not been enumerated -- e.g. U+2705 white-heavy-check-mark.
PREFIX = re.compile(r"^(?:\s+|>|\#{1,6}|\d+[.)]|[-+]\s|\*\s|\||~~"
                    r"|[←-⯿️\U0001F300-\U0001FAFF])")
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

# Dangling tags inside Vaino v1 documents that are themselves on the disposal
# path [GDE-DIS-010]. Recorded rather than fixed: editing a document slated for
# replacement is churn. Retires with the document.
PREEXISTING_V1_DANGLING = {"SPEC-AUD-050", "REQ-SYS-010", "REQ-SYS-020",
                           "REQ-MB-020", "REQ-PD-080", "REQ-HW-010", "REQ-HW-020",
                           "SPEC-AUD-010", "SPEC-DB-010", "SPEC-RUST-010"}

# Tags used illustratively in GOV001's taxonomy table -- examples of the FORM
# of an identifier, not references to real ones.
EXAMPLE_TAGS = {"REQ-AUD-010", "REQ-DB-020", "SPEC-AUD-020", "SPEC-AUD-040",
                "SPEC-PD-010", "ENT-TRACK-010", "ENT-PASSAGE-010",
                "UT-AUD-001", "UT-DB-001", "GOV-DOC-010"}


def strip_code(text):
    return CODESPAN.sub("", text)


def strip_lead(line):
    prev = None
    while prev != line:
        prev = line
        line = PREFIX.sub("", line, count=1)
    return line


def is_definition(line, tag):
    """A definition OPENS a line; a citation sits mid-sentence.

    Deliberately conservative. An earlier heuristic accepted any bolded line
    containing the tag and reported 94 duplicate definitions, nearly all false.
    Index rows cannot be excluded by shape -- real definitions also live in
    multi-column tables -- so REFERENCE_SECTIONS names them instead.
    """
    return bool(re.match(r"(?:\*\*)?`?\[" + re.escape(tag) + r"\]`?", strip_lead(line)))


# Sections that INDEX tags defined elsewhere. Structure alone cannot distinguish
# these from definitions -- real definitions also live in multi-column tables
# (the lessons and risks tables) -- so they are named explicitly.
REFERENCE_SECTIONS = ("master specification search index", "identifier taxonomy standard")


def in_reference_section(heading):
    return heading and any(k in heading.lower() for k in REFERENCE_SECTIONS)


def vaino_docs():
    """Vaino-authored markdown. docs/inherited/README.md is ours, not inherited."""
    out = glob.glob("docs/*.md") + glob.glob("docs/spec/*.md")
    out = [p for p in out if INHERITED_DIR not in p]
    reg = os.path.join(INHERITED_DIR, "README.md")
    if os.path.exists(reg):
        out.append(reg)
    return out


def inherited_docs():
    return [p for p in glob.glob(os.path.join(INHERITED_DIR, "**", "*.md"), recursive=True)
            if os.path.basename(p) != "README.md"]


def tags_in(path, skip_banner=False):
    """Tags in a document.

    skip_banner drops ONLY the import banner (everything above the first
    horizontal rule), not every blockquote. Source documents legitimately put
    tags in blockquotes -- McRhythm's SPEC003 defines [MFL-DIST-010] in one --
    and dropping those made real tags appear undefined.
    """
    text = open(path, encoding="utf-8").read()
    if skip_banner and text.lstrip().startswith(">"):
        parts = text.split("\n---\n", 1)
        if len(parts) == 2:
            text = parts[1]
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

    # 3 -- exact tag collisions: a collision is two DEFINITIONS, not a
    # citation. Vaino documents legitimately cite inherited tags such as
    # [MFL-DEF-040] and [ENT-MP-030]; that is correct cross-referencing, not a
    # namespace clash. Comparing raw presence flagged those as errors.
    def definitions(paths, skip_banner=False):
        out = {}
        for path in paths:
            text = open(path, encoding="utf-8").read()
            if skip_banner and text.lstrip().startswith(">"):
                parts = text.split("\n---\n", 1)
                if len(parts) == 2:
                    text = parts[1]
            for line in text.splitlines():
                for t in set(TAG.findall(line)):
                    if is_definition(line, t):
                        out.setdefault(t, []).append(path)
        return out

    vdef = definitions(vaino_docs())
    idef = definitions(inherited_docs(), skip_banner=True)
    for t in sorted(set(vdef) & set(idef)):
        if t in KNOWN_COLLISIONS:
            warnings.append(f"[INH-HAZ-020] known collision {t} "
                            f"(retires when {COLLISION_RETIRES_WHEN})")
            continue
        errors.append(f"[INH-HAZ-020] tag {t} DEFINED in both "
                      f"{os.path.basename(vdef[t][0])} and {os.path.basename(idef[t][0])}")

    # 4 -- relative links resolve
    for p in glob.glob("docs/**/*.md", recursive=True):
        root = os.path.dirname(p)
        for m in LINK.finditer(strip_code(open(p, encoding="utf-8").read())):
            target = os.path.normpath(os.path.join(root, m.group(1).split("#")[0]))
            if not os.path.exists(target):
                errors.append(f"broken link: {p} -> {m.group(1)}")

    # 6 -- tag definition integrity
    inherited_tags = set()
    for p in inherited_docs():
        inherited_tags |= tags_in(p, skip_banner=True)
    defs, uses = {}, {}
    for p in vaino_docs():
        heading, fenced = "", False
        for n, line in enumerate(open(p, encoding="utf-8"), 1):
            if line.lstrip().startswith("```"):
                fenced = not fenced
            if fenced:
                continue
            if line.lstrip().startswith("#"):
                heading = line.strip("#").strip()
            ref = in_reference_section(heading)
            for t in set(TAG.findall(line)):
                d = is_definition(line, t) and not ref
                (defs if d else uses).setdefault(t, []).append(f"{p}:{n}")
    for t, locs in sorted(uses.items()):
        if t in defs or t in inherited_tags or t in EXAMPLE_TAGS:
            continue
        if t in PREEXISTING_V1_DANGLING:
            warnings.append(f"pre-existing v1 dangling tag {t} at {locs[0]} "
                            f"(retires with the document, [GDE-DIS-010])")
            continue
        errors.append(f"dangling tag {t} cited at {locs[0]} but never defined")
    for t, locs in sorted(defs.items()):
        if len(locs) > 1 and t not in KNOWN_COLLISIONS:
            warnings.append(f"tag {t} defined {len(locs)}x: {', '.join(locs)}")

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
