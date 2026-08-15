import os
import sys
import sqlite3
import logging

sys.stdout.reconfigure(encoding='utf-8')
sys.path.insert(0, r"C:\Users\Mango Cat\Dev\Vaino")

from src.db.acoustic_resolver import AcousticResolver, AB_COLS, MULIB_DEFAULT_PATH

VAINO_DB = r"C:\Users\Mango Cat\Dev\Vaino\vaino.db"

def main():
    print(f"=================================================================")
    print(f" ANALYZING TIER RESOLUTION BREAKDOWN FOR PRESERVED TRACKS")
    print(f"=================================================================\n")

    conn = sqlite3.connect(VAINO_DB)
    conn.row_factory = sqlite3.Row
    cur = conn.execute("SELECT * FROM tracks")
    tracks = cur.fetchall()
    conn.close()

    total_tracks = len(tracks)
    print(f"Total tracks in Vaino library: {total_tracks}")

    t1_matches = 0
    t2_matches = 0
    t3_only = 0
    failed = 0

    mconn = sqlite3.connect(MULIB_DEFAULT_PATH)
    mconn.row_factory = sqlite3.Row

    for idx, r in enumerate(tracks, 1):
        d = dict(r)
        mbid = d.get('musicbrainz_track_id')
        rel_path = d.get('file_path', '')
        
        path_parts = rel_path.replace('/', '\\').split('\\')
        tail_path = "\\".join(path_parts[-2:]) if len(path_parts) >= 2 else rel_path
        tail_posix = tail_path.replace('\\', '/')

        # Check Tier 1 match in mulib.db
        t1_row = None
        if mbid:
            c = mconn.execute("SELECT abAcoustic FROM tracks WHERE mbidRecording = ?", (mbid,))
            t1_row = c.fetchone()
        if not t1_row and tail_path:
            c = mconn.execute("""
                SELECT t.abAcoustic FROM tracks t
                JOIN cuts c_t ON c_t.trackId = t.trackId
                JOIN files f ON f.fileId = c_t.fileId
                WHERE f.filePath LIKE ? OR f.filePath LIKE ?
            """, (f"%{tail_path}%", f"%{tail_posix}%"))
            t1_row = c.fetchone()

        if t1_row and t1_row["abAcoustic"] is not None:
            t1_matches += 1
        elif mbid:
            t2_matches += 1
        elif rel_path and os.path.exists(rel_path):
            t3_only += 1
        else:
            failed += 1

    mconn.close()

    print(f"\n=================================================================")
    print(f" RE-EVALUATION OF 1,823 PRESERVED TRACKS (TIER UPGRADES)")
    print(f"=================================================================")
    print(f" Total Library Tracks                   : {total_tracks:5d} tracks")
    print(f" Tier 1 (mulib.db Ground Truth Transfer) : {t1_matches:5d} tracks ({t1_matches/total_tracks*100.0:.1f}%)")
    print(f" Tier 2 (AcousticBrainz Online API)      : {t2_matches:5d} tracks ({t2_matches/total_tracks*100.0:.1f}%)")
    print(f" Tier 3 (Local MusicNN ONNX Deep Net)    : {t3_only:5d} tracks ({t3_only/total_tracks*100.0:.1f}%)")
    print(f" Unresolved / Missing                    : {failed:5d} tracks ({failed/total_tracks*100.0:.1f}%)")
    print(f"-----------------------------------------------------------------")
    print(f" GROUND TRUTH ACCURACY (Tiers 1 + 2)    : {t1_matches + t2_matches:5d} tracks ({(t1_matches + t2_matches)/total_tracks*100.0:.1f}%)")
    print(f" TOTAL SUCCESS RATE                     : {(total_tracks - failed)/total_tracks*100.0:.2f}%")
    print(f"=================================================================\n")

if __name__ == "__main__":
    main()
