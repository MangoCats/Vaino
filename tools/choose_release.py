#!/usr/bin/env python3
"""Sampo S3, selection half: which release is a file actually from?

A recording appears on many releases -- 86 of them for one track in this
library -- and more than half of all candidates are compilations. "Earliest by
date" is close to a coin toss against a pool like that, and it is how a song
ends up filed under a greatest-hits collection it was never written for.

The criteria are McRhythm's `[AM-MB-020]`/`[AM-MB-030]`/`[AM-MB-040]`, adapted.
McRhythm was matching a whole album rip with no identifiers, so it searched by
name across seven strategies and scored artist similarity at 60% against album
at 40% -- artist being the more stable of the two across reissues. Here the
artist is already fixed by the recording MBID, so that term is spent, and what
remains is:

  * the file's own ALBUM tag against the release title (5,587 files have one),
    which is direct evidence of the record it was ripped from;
  * kind -- an Album that is not a compilation, live album or soundtrack;
  * status -- Official over Promotion over Bootleg;
  * date, last, as the tiebreak McRhythm's cascade also left until last.

Nothing is discarded. The choice, its margin and its runners-up are written to
`ingest_decisions` `[REQ-VIS-110]`, because a selection nobody can argue with is
a selection nobody can correct.

    python tools/choose_release.py data/vaino_new.db [--limit N] [--explain MBID]
"""

import argparse
import json
import sqlite3
import sys
import time

# Weights. Deliberately blunt: the name term decides when a tag exists, and the
# kind term decides when it does not. Tuning these past one decimal place would
# be fitting noise -- there is no labelled truth to fit against.
W_NAME = 5.0
W_KIND = 3.0
W_STATUS = 1.0
W_DATE = 0.5

# What disqualifies a release from being "the record this song is from". Live
# and soundtrack are included because a studio track appearing on either is
# almost never the release it belongs to.
DEMOTE = {"Compilation": -1.0, "Live": -0.7, "Soundtrack": -0.6,
          "Remix": -0.5, "DJ-mix": -0.8, "Interview": -1.0, "Demo": -0.3}


def jaro_winkler(a: str, b: str) -> float:
    """Similarity in [0, 1]. Pure Python: one file, no dependencies.

    Winkler's prefix bonus is what makes it right for album titles -- "Aja" and
    "Aja (Remastered)" should read as near-identical, and they do.
    """
    if not a or not b:
        return 0.0
    a, b = a.lower(), b.lower()
    if a == b:
        return 1.0
    reach = max(len(a), len(b)) // 2 - 1
    if reach < 0:
        reach = 0
    a_hit = [False] * len(a)
    b_hit = [False] * len(b)
    matches = 0
    for i, ch in enumerate(a):
        for j in range(max(0, i - reach), min(len(b), i + reach + 1)):
            if not b_hit[j] and b[j] == ch:
                a_hit[i] = b_hit[j] = True
                matches += 1
                break
    if not matches:
        return 0.0
    # Transpositions: matched characters that arrive in a different order.
    k = transpositions = 0
    for i, ch in enumerate(a):
        if a_hit[i]:
            while not b_hit[k]:
                k += 1
            if ch != b[k]:
                transpositions += 1
            k += 1
    m = float(matches)
    jaro = (m / len(a) + m / len(b) + (m - transpositions / 2) / m) / 3
    prefix = 0
    for x, y in zip(a[:4], b[:4]):
        if x != y:
            break
        prefix += 1
    return jaro + prefix * 0.1 * (1 - jaro)


def kind_score(primary: str | None, secondary: str | None) -> float:
    """How much this looks like the record a song belongs to."""
    score = 1.0 if primary == "Album" else (0.4 if primary == "EP" else 0.0)
    for tag in (secondary or "").split(","):
        if tag.strip() in DEMOTE:
            score += DEMOTE[tag.strip()]
    return score


def status_score(status: str | None) -> float:
    return {"Official": 1.0, "Promotion": 0.3, "Bootleg": -0.5}.get(status, 0.5)


def year(date: str | None) -> int | None:
    if not date or not date[:4].isdigit():
        return None
    return int(date[:4])


def score_all(rows, album_tag: str | None, oldest: int | None):
    """Score every candidate, best first. `rows` are the release columns."""
    out = []
    for r in rows:
        rid, title, date, status, primary, secondary, track_count = r
        name = jaro_winkler(album_tag or "", title or "")
        kind = kind_score(primary, secondary)
        stat = status_score(status)
        # Earlier is better, but gently: the original pressing usually is the
        # record, and a remaster thirty years later usually is not.
        y = year(date)
        age = 0.0
        if y and oldest:
            age = max(0.0, 1.0 - (y - oldest) / 30.0)
        total = (W_NAME * name + W_KIND * kind + W_STATUS * stat + W_DATE * age)
        out.append({
            "release": rid, "title": title, "date": date, "status": status,
            "primary_type": primary, "secondary_types": secondary,
            "track_count": track_count,
            "name_similarity": round(name, 3), "kind": round(kind, 2),
            "score": round(total, 3),
        })
    # Ordered criteria, not a weighted sum -- which is how McRhythm ranked
    # `[AM-MB-040]`, and the reason matters. A sum lets `kind` outvote a name
    # that matched almost exactly: a release titled the same as the file's tag
    # at 0.99 lost to an unrelated album at 0.48, because "is an album" was
    # worth more than "is the record this file names".
    #
    # So a confident tag match settles it, and everything else only breaks ties
    # WITHIN that tier. The tag is evidence of the edition the file came from,
    # which is the question being asked; whether that edition is a compilation
    # is not the question.
    best_name = max((d["name_similarity"] for d in out), default=0.0)
    if best_name >= 0.85:
        tier = [d for d in out if d["name_similarity"] >= best_name - 0.05]
    else:
        tier = out
    tier.sort(key=lambda d: (-d["score"], d["date"] or "9999", d["title"] or ""))
    rest = sorted((d for d in out if d not in tier),
                  key=lambda d: (-d["score"], d["date"] or "9999"))
    return tier + rest


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("db")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--explain", help="show the scoring for one recording MBID")
    args = ap.parse_args()

    conn = sqlite3.connect(args.db)
    conn.execute("PRAGMA busy_timeout = 5000")
    # The chosen flag lives beside the link it qualifies. Vaino reads it with
    # `ORDER BY chosen DESC`, which still behaves when nothing has been chosen.
    try:
        conn.execute("ALTER TABLE release_recordings ADD COLUMN chosen INTEGER DEFAULT 0")
    except sqlite3.OperationalError:
        pass
    conn.execute("""CREATE TABLE IF NOT EXISTS ingest_decisions (
        decision_id INTEGER PRIMARY KEY, audio_md5 TEXT, stage TEXT,
        outcome TEXT, confidence REAL, detail TEXT, decided_at INTEGER)""")

    mbids = [r[0] for r in conn.execute(
        "SELECT DISTINCT mbid FROM release_recordings ORDER BY mbid")]
    if args.explain:
        mbids = [args.explain]
    elif args.limit:
        mbids = mbids[: args.limit]

    decided = tagged = 0
    now = int(time.time())
    for mbid in mbids:
        rows = conn.execute(
            "SELECT rel.mbid, rel.title, rel.release_date, rel.status, "
            "       rel.primary_type, rel.secondary_types, rel.track_count "
            "  FROM release_recordings rr JOIN releases rel ON rel.mbid = rr.release_mbid "
            " WHERE rr.mbid = ?1", (mbid,)).fetchall()
        if not rows:
            continue
        # The file's own album tag, via any passage using this recording.
        tag = conn.execute(
            "SELECT ft.album, f.audio_md5 FROM passage_recordings pr "
            "  JOIN passages p ON p.passage_id = pr.passage_id "
            "  JOIN files f ON f.file_id = p.file_id "
            "  LEFT JOIN file_tags ft ON ft.file_id = f.file_id "
            " WHERE pr.mbid = ?1 LIMIT 1", (mbid,)).fetchone()
        album_tag, md5 = (tag or (None, None))
        if album_tag:
            tagged += 1
        years = [year(r[2]) for r in rows]
        oldest = min([y for y in years if y], default=None)

        ranked = score_all(rows, album_tag, oldest)
        if args.explain:
            print(f"recording {mbid}   file's album tag: {album_tag!r}")
            for d in ranked[:8]:
                print(f"  {d['score']:6.2f}  name {d['name_similarity']:.2f} "
                      f"kind {d['kind']:+.2f}  {str(d['date'])[:4]:>4}  "
                      f"{(d['primary_type'] or '?')}/{d['secondary_types'] or '-'}  "
                      f"{d['title']}")
            return 0

        best = ranked[0]
        runner = ranked[1] if len(ranked) > 1 else None
        conn.execute("UPDATE release_recordings SET chosen = 0 WHERE mbid = ?1", (mbid,))
        conn.execute("UPDATE release_recordings SET chosen = 1 "
                     " WHERE mbid = ?1 AND release_mbid = ?2", (mbid, best["release"]))
        # The margin is the part worth keeping: a win by 0.02 over a compilation
        # is a coin toss that someone should look at, and a win by 4 is not.
        conn.execute(
            "INSERT INTO ingest_decisions "
            "  (audio_md5, stage, outcome, confidence, detail, decided_at) "
            "VALUES (?1, 'release_match', ?2, ?3, ?4, ?5)",
            (md5, best["release"],
             round(best["score"] - (runner["score"] if runner else 0.0), 3),
             json.dumps({"recording": mbid, "album_tag": album_tag,
                         "candidates": len(ranked), "chosen": best,
                         "runners_up": ranked[1:4]}),
             now))
        decided += 1
        if decided % 200 == 0:
            conn.commit()
            print(f"  {decided}/{len(mbids)}", flush=True)

    conn.commit()
    print(f"chose a release for {decided} recording(s); {tagged} had an album tag to "
          f"match against")
    return 0


if __name__ == "__main__":
    sys.exit(main())
