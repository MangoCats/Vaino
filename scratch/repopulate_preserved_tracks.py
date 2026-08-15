import os
import sys
import time
import sqlite3
import logging

sys.stdout.reconfigure(encoding='utf-8')
sys.path.insert(0, r"C:\Users\Mango Cat\Dev\Vaino")

from src.db.database import Database
from src.db.acoustic_resolver import AcousticResolver, AB_COLS, MULIB_DEFAULT_PATH

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger(__name__)

VAINO_DB = r"C:\Users\Mango Cat\Dev\Vaino\vaino.db"

def main():
    print(f"=================================================================")
    print(f" RE-RUNNING 3-TIER RESOLVER ON PREVIOUSLY PRESERVED TRACKS")
    print(f" Target Database: {VAINO_DB}")
    print(f" Ground Truth DB: {MULIB_DEFAULT_PATH}")
    print(f"=================================================================\n")

    conn = sqlite3.connect(VAINO_DB)
    conn.row_factory = sqlite3.Row

    # Fetch all tracks
    cur = conn.execute("SELECT * FROM tracks")
    all_tracks = cur.fetchall()
    conn.close()

    print(f"Loaded {len(all_tracks)} total tracks from Vaino library.")

    # Identify the 1,823 tracks that had pre-existing data prior to batch run
    # We test each track against Tier 1 and Tier 2 to upgrade any Tier 3/heuristic data
    upgraded_t1 = 0
    upgraded_t2 = 0
    retained_t3 = 0
    failed_count = 0

    start_time = time.time()
    conn = sqlite3.connect(VAINO_DB)

    print(f"\nEvaluating tracks for Tier 1 / Tier 2 ground truth upgrades...\n")

    for idx, track_row in enumerate(all_tracks, 1):
        d = dict(track_row)
        title = d.get('title', 'Unknown Title')
        file_path = d.get('file_path')
        mbid = d.get('musicbrainz_track_id')

        clean_title = title.encode('ascii', errors='replace').decode('ascii')

        # Attempt Tier 1 upgrade
        t1_res = AcousticResolver._try_tier1_mulib(d, MULIB_DEFAULT_PATH)
        if t1_res:
            AcousticResolver._update_db_track(conn, d['id'], t1_res)
            upgraded_t1 += 1
            if idx % 500 == 0:
                print(f"[{idx:4d}/{len(all_tracks)}] Processed... (Tier 1 upgrades so far: {upgraded_t1})")
            continue

        # Attempt Tier 2 upgrade
        if mbid:
            t2_res = AcousticResolver._try_tier2_api(mbid)
            if t2_res:
                AcousticResolver._update_db_track(conn, d['id'], t2_res)
                upgraded_t2 += 1
                print(f"[{idx:4d}/{len(all_tracks)}] Upgraded to Tier 2 (API): {clean_title[:35]}")
                continue

        # Tier 3 (MusicNN ONNX) fallback
        if file_path and os.path.exists(file_path):
            t3_res = AcousticResolver._try_tier3_onnx(file_path)
            if t3_res:
                AcousticResolver._update_db_track(conn, d['id'], t3_res)
                retained_t3 += 1
                continue

        failed_count += 1

        if idx % 200 == 0:
            conn.commit()

    conn.commit()
    conn.close()

    elapsed = time.time() - start_time

    print(f"\n=================================================================")
    print(f" FINAL RE-EVALUATED 3-TIER RESOLVER BREAKDOWN ({len(all_tracks)} TRACKS)")
    print(f"=================================================================")
    print(f" Tier 1 (mulib.db Ground Truth Transfer) : {upgraded_t1:5d} tracks ({upgraded_t1/len(all_tracks)*100.0:.1f}%)")
    print(f" Tier 2 (AcousticBrainz Online API)      : {upgraded_t2:5d} tracks ({upgraded_t2/len(all_tracks)*100.0:.1f}%)")
    print(f" Tier 3 (Local MusicNN ONNX Deep Net)    : {retained_t3:5d} tracks ({retained_t3/len(all_tracks)*100.0:.1f}%)")
    print(f" Unresolved / Failed Tracks              : {failed_count:5d} tracks ({failed_count/len(all_tracks)*100.0:.1f}%)")
    print(f"-----------------------------------------------------------------")
    print(f" TOTAL GROUND TRUTH COVERAGE (Tiers 1 + 2): {upgraded_t1 + upgraded_t2:5d} tracks ({(upgraded_t1+upgraded_t2)/len(all_tracks)*100.0:.1f}%)")
    print(f" TOTAL SUCCESS RATE                       : {(len(all_tracks)-failed_count)/len(all_tracks)*100.0:.2f}%")
    print(f" TOTAL ELAPSED TIME                       : {elapsed:.2f} seconds")
    print(f"=================================================================\n")

if __name__ == "__main__":
    main()
