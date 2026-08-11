"""Extract lowlevel features and classify them into flavor `[GDE-FEX-102]`.

Two stages, deliberately separate:

  audio → streaming_extractor_music → lowlevel JSON → `lowlevel_cache`
  cache → 18 Gaia chains          → 71 dimensions  → `flavor`

Extraction is the only expensive step (~27 s/track) and the only one needing
audio, so it caches against `audio_md5` `[SPEC-SC-080]`: improving a classifier
re-runs stage two over the cache and never re-decodes the library.

Values are written with `source = 'local:<extractor>+gaia'`. Provenance must
stay uniform across a library `[SPEC-FD-145]` -- mixing local and inherited
values costs ~8 points of retrieval accuracy -- so a partial run is for
measurement, never for listening.

Usage:
  python tools/extract_library.py <vaino.db> [--limit N] [--jobs N]
"""

from __future__ import annotations

import concurrent.futures as futures
import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import gaia_classify as gc  # noqa: E402

EXTRACTOR = Path("data/essentia/streaming_extractor_music.exe")
SOURCE = "local:essentia-2.1-beta2+gaia-beta1"


def extract_one(path: str) -> dict | None:
    """Run the reference extractor on one file. `None` if it fails."""
    if not Path(path).exists():
        return None
    tmp = Path(tempfile.gettempdir()) / f"ll_{os.getpid()}_{abs(hash(path))}.json"
    try:
        r = subprocess.run(
            [str(EXTRACTOR), path, str(tmp)],
            capture_output=True,
            timeout=300,
        )
        if r.returncode != 0 or not tmp.exists():
            return None
        return json.loads(tmp.read_text(encoding="utf-8", errors="replace"))
    except (subprocess.TimeoutExpired, json.JSONDecodeError, OSError):
        return None
    finally:
        tmp.unlink(missing_ok=True)


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    db = Path(args[0])
    limit = int(args[args.index("--limit") + 1]) if "--limit" in args else 0
    jobs = int(args[args.index("--jobs") + 1]) if "--jobs" in args else max(1, (os.cpu_count() or 4) - 1)

    con = sqlite3.connect(db)
    con.execute(
        """CREATE TABLE IF NOT EXISTS lowlevel_cache (
             audio_md5 TEXT NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
             features BLOB NOT NULL, extractor TEXT NOT NULL, extracted_at TEXT NOT NULL,
             PRIMARY KEY (audio_md5, start_ms, end_ms))"""
    )
    done = {r[0] for r in con.execute("SELECT audio_md5 FROM lowlevel_cache")}

    rows = con.execute(
        "SELECT f.audio_md5, f.path, pr.mbid FROM files f "
        "JOIN passages p USING (file_id) JOIN passage_recordings pr USING (passage_id) "
        "WHERE p.kind = 'radio' GROUP BY f.audio_md5"
    ).fetchall()
    todo = [r for r in rows if r[0] not in done]
    if limit:
        todo = todo[:limit]
    print(f"{len(rows)} files, {len(done)} cached, {len(todo)} to extract, {jobs} jobs")
    if not todo:
        return 0

    clf = gc.Classifier()
    t0 = time.time()
    ok = fail = 0
    with futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        pending = {pool.submit(extract_one, r[1]): r for r in todo}
        for i, fut in enumerate(futures.as_completed(pending), 1):
            md5, path, mbid = pending[fut]
            doc = fut.result()
            if doc is None:
                fail += 1
                continue
            con.execute(
                "INSERT OR REPLACE INTO lowlevel_cache VALUES (?,?,?,?,?,datetime('now'))",
                (md5, 0, -1, zlib.compress(json.dumps(doc).encode()), "essentia-2.1-beta2"),
            )
            for ch, classes in clf.classify(doc).items():
                for cls, val in classes.items():
                    con.execute(
                        "INSERT OR REPLACE INTO flavor VALUES ('recording',?,?,?,?,?,NULL)",
                        (mbid, ch, cls, val, SOURCE),
                    )
            ok += 1
            if i % 10 == 0:
                el = time.time() - t0
                con.commit()
                print(f"  {i}/{len(todo)}  ok={ok} fail={fail}  "
                      f"{el/i:.1f}s/track  eta {(len(todo)-i)*el/i/60:.0f} min", flush=True)
    con.commit()
    el = time.time() - t0
    print(f"\n{ok} extracted, {fail} failed in {el/60:.1f} min "
          f"({el/max(ok,1):.1f}s/track wall, {jobs} jobs)")
    print(f"full library estimate: {len(rows)*el/max(ok,1)/3600:.1f} h at this rate")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
