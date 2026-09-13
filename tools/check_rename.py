#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Rename audit `[IMPL-NAM-020]`: what still carries the old name, by surface.

Written BEFORE the Vaino -> Lempi rename begins, deliberately, while the old
name is still everywhere. An audit written afterwards that reports zero is
indistinguishable from an audit that is broken; this one is committed while it
still has something to find, so its first run proves it works.

The surfaces below are not a text search. `grep -ri vaino` counts strings; this
counts the *things that break* -- a crate path, a compile-time env var, an
installed executable, a data directory on a live appliance, a hostname -- and
reports them separately so a step can be gated on one surface reaching zero
without waiting for the others.

**A surface whose file set is empty reports BROKEN, not zero** `[GDE-DEP-060]`.
A glob that matches nothing after a directory rename would otherwise report
clean, which is how `tools/check_docs.py`'s own `PATH_PREFIXES` would have
silently stopped checking `VainoPi/` -- see `[IMPL-NAM-060]`.

Usage:
    python tools/check_rename.py                      # report every surface
    python tools/check_rename.py --expect-zero code env
    python tools/check_rename.py --verbose            # list every hit
"""

import argparse
import glob
import os
import re
import subprocess
import sys

OLD = re.compile(r"vaino", re.IGNORECASE)

# Files where the old name is correct and must survive the rename: the record
# of why it changed, the lineage documents, and this script. Listing them is
# not a convenience -- an audit that cannot distinguish a live reference from a
# historical one produces a number nobody can act on.
ALLOW = {
    "tools/check_rename.py",
    "docs/GUIDE015-naming-and-branding.md",
    "docs/IMPL013-executing-the-rename.md",
    "docs/GUIDE001-lineage-and-lessons.md",
}

# (name, globs, pattern, what breaks if this is non-zero at cutover)
SURFACES = [
    ("cargo", ["player/Cargo.toml", "player/Cargo.lock"],
     r"vaino[-_]player|name\s*=\s*\"vaino\"",
     "crate and binary names; every ExecStart and install path downstream"),
    ("code", ["player/src/**/*.rs", "player/tests/*.rs",
              "player/examples/*.rs", "player/build.rs"],
     r"vaino[-_]player|vaino_player",
     "library crate path; the build fails until every use site moves"),
    ("env", ["player/src/**/*.rs", "player/build.rs", "build/*.sh",
             "VainoPi/*.sh", "BosePi/*.sh"],
     # Case-sensitive deliberately: a bare VAINO_[A-Z_]+ under IGNORECASE also
     # matches `vaino_player`, which double-counts the `code` surface. Found by
     # running this script against the tree it was written for.
     r"(?-i:VAINO_[A-Z_]+)",
     "VAINO_NULL_OUTPUT and friends read via env::var().is_ok() -- a half "
     "rename does not error, it silently takes the real-audio path"),
    ("bin", ["build/*.sh", "BosePi/*.sh", "VainoPi/*.sh", "BosePi/*.service"],
     r"/usr/local/bin/vaino[a-z-]*",
     "the installed executables, main and the thirteen vaino-* helpers"),
    ("units", ["BosePi/*.service", "VainoPi/*.service"],
     r"vaino",
     "systemd units; an old enabled unit contends for the audio device"),
    ("runtime", ["build/*.sh", "BosePi/*.sh", "VainoPi/*.sh",
                 "BosePi/*.service"],
     r"/var/vaino|/srv/library/vaino",
     "data directories on live appliances -- a migration, not an edit"),
    ("hosts", ["build/*.sh", "BosePi/*.sh", "VainoPi/*.sh"],
     r"vainopi|vainoplayer3",
     "deploy targets; these are renamed per machine, never atomically"),
    ("scripts", ["build/*.sh", "BosePi/*.sh", "VainoPi/*.sh"],
     r"vaino", "everything the scripts say and do"),
    ("pytools", ["tools/*.py"], r"vaino|VainoPi",
     "check_docs.py's PATH_PREFIXES above all -- see [IMPL-NAM-060]"),
    ("docs", ["docs/**/*.md", "VainoPi/*.md", "BosePi/*.md", "SmartPC/*.md",
              "sendspin/*.md", "*.md"],
     r"vaino", "prose, and the cited paths check_docs.py validates"),
]


def tracked_files(patterns):
    out = []
    for pat in patterns:
        out.extend(glob.glob(pat, recursive=True))
    return sorted({p.replace("\\", "/") for p in out if os.path.isfile(p)})


def scan(name, patterns, pattern, verbose):
    """Return (count, files_hit, broken). broken=True means nothing to scan."""
    files = tracked_files(patterns)
    if not files:
        return 0, [], True
    rx = re.compile(pattern, re.IGNORECASE)
    count, hits = 0, []
    for path in files:
        if path in ALLOW:
            continue
        try:
            with open(path, encoding="utf-8", errors="replace") as fh:
                for lineno, line in enumerate(fh, 1):
                    found = rx.findall(line)
                    if found:
                        count += len(found)
                        if verbose:
                            hits.append(f"    {path}:{lineno}: {line.strip()[:100]}")
                        elif path not in hits:
                            hits.append(path)
        except OSError as exc:
            print(f"ERROR  {name}: cannot read {path}: {exc}", file=sys.stderr)
            return count, hits, True
    return count, hits, False


def named_paths():
    """Files and directories whose own name carries the old name."""
    try:
        listing = subprocess.run(["git", "ls-files"], capture_output=True,
                                 text=True, check=True).stdout.splitlines()
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"ERROR  path surface: git ls-files failed: {exc}", file=sys.stderr)
        return None
    return [p for p in listing if OLD.search(p)]


def git_remote():
    try:
        url = subprocess.run(["git", "remote", "get-url", "origin"],
                             capture_output=True, text=True, check=True).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        return None
    return url


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--expect-zero", nargs="*", default=[], metavar="SURFACE",
                    help="fail if any named surface is non-zero")
    ap.add_argument("--verbose", action="store_true", help="list every hit")
    args = ap.parse_args()

    known = {s[0] for s in SURFACES} | {"paths", "remote"}
    unknown = set(args.expect_zero) - known
    if unknown:
        print(f"ERROR  unknown surface(s): {', '.join(sorted(unknown))}")
        print(f"       known: {', '.join(sorted(known))}")
        return 2

    results, broken = {}, []
    for name, patterns, pattern, consequence in SURFACES:
        count, hits, is_broken = scan(name, patterns, pattern, args.verbose)
        results[name] = count
        if is_broken:
            broken.append(name)
            print(f"BROKEN {name:9} matched no files at all -- this surface is "
                  f"NOT being checked. Fix the globs before trusting any run.")
            continue
        print(f"{count:6}  {name:9} {consequence}")
        if hits and args.verbose:
            print("\n".join(hits[:40]))
        elif hits:
            print(f"          in {len(hits)} file(s): {', '.join(hits[:4])}"
                  + (" ..." if len(hits) > 4 else ""))

    paths = named_paths()
    if paths is None:
        broken.append("paths")
    else:
        results["paths"] = len(paths)
        print(f"{len(paths):6}  {'paths':9} tracked files/dirs whose own name "
              f"carries it")
        if paths:
            print(f"          e.g. {', '.join(paths[:3])}"
                  + (" ..." if len(paths) > 3 else ""))

    url = git_remote()
    if url is None:
        broken.append("remote")
        print("BROKEN remote    could not read origin URL")
    else:
        results["remote"] = 1 if OLD.search(url) else 0
        print(f"{results['remote']:6}  {'remote':9} origin is {url}")

    print()
    total = sum(results.values())
    # Surfaces overlap on purpose -- `scripts` is a superset of `bin`, `units`,
    # `runtime` and `hosts`, so a step can be gated on the narrow surface it
    # actually moves. The total is therefore a workload figure, not a count of
    # distinct occurrences, and must not be quoted as one.
    print(f"{total} occurrence(s) across {len(results)} surface(s), which "
          f"overlap; {len(broken)} surface(s) BROKEN")

    if broken:
        print(f"FAIL   broken surface(s): {', '.join(broken)} -- a surface that "
              f"cannot run must not be read as clean")
        return 1

    failed = [s for s in args.expect_zero if results.get(s, 0) != 0]
    if failed:
        for s in failed:
            print(f"FAIL   {s} expected 0, found {results[s]}")
        return 1
    if args.expect_zero:
        print(f"OK     {', '.join(args.expect_zero)} at zero")
    return 0


if __name__ == "__main__":
    sys.exit(main())
