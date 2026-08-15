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
    print(f" EXECUTING 3-TIER ACOUSTIC RESOLVER ON VAINO MUSIC LIBRARY")
    print(f" Target Database: {VAINO_DB}")
    print(f" Ground Truth DB: {MULIB_DEFAULT_PATH}")
    print(f"=================================================================\n")

    conn = sqlite3.connect(VAINO_DB)
    conn.row_factory = sqlite3.Row

    # Fetch all tracks from Vaino library
    cur = conn.execute("SELECT * FROM tracks")
    tracks = cur.fetchall()
    total_tracks = len(tracks)

    print(f"Loaded {total_tracks} total tracks from Vaino library database.\n")

    t1_count = 0
    t2_count = 0
    t3_count = 0
    existing_count = 0
    fail_count = 0

    start_time = time.time()

    for idx, track_row in enumerate(tracks, 1):
        d = dict(track_row)
        title = d.get('title', 'Unknown Title')
        artist = d.get('artist', 'Unknown Artist')
        file_path = d.get('file_path')
        mbid = d.get('musicbrainz_track_id')

        # Check if already populated
        has_all = all(d.get(c) is not None for c in AB_COLS)
        if has_all:
            existing_count += 1
            continue

        clean_title = title.encode('ascii', errors='replace').decode('ascii')
        print(f"[{idx:4d}/{total_tracks}] Processing: {clean_title[:38]:<38} ... ", end="", flush=True)

        # 1. Tier 1: mulib.db lookup
        t1_res = AcousticResolver._try_tier1_mulib(d, MULIB_DEFAULT_PATH)
        if t1_res:
            AcousticResolver._update_db_track(conn, d['id'], t1_res)
            t1_count += 1
            print("TIER 1 (mulib.db)")
            continue

        # 2. Tier 2: AcousticBrainz API lookup
        if mbid:
            t2_res = AcousticResolver._try_tier2_api(mbid)
            if t2_res:
                AcousticResolver._update_db_track(conn, d['id'], t2_res)
                t2_count += 1
                print("TIER 2 (AcousticBrainz API)")
                continue

        # 3. Tier 3: Local MTG Essentia MusicNN ONNX Neural Net Inference
        if file_path and os.path.exists(file_path):
            t3_res = AcousticResolver._try_tier3_onnx(file_path)
            if t3_res:
                AcousticResolver._update_db_track(conn, d['id'], t3_res)
                t3_count += 1
                print("TIER 3 (Local ONNX Deep Net)")
                continue

        fail_count += 1
        print("FAILED (No ground truth, MBID, or local file)")

        if idx % 100 == 0:
            conn.commit()

    conn.commit()
    conn.close()

    elapsed = time.time() - start_time
    total_successful = t1_count + t2_count + t3_count + existing_count
    success_rate = (total_successful / total_tracks * 100.0) if total_tracks > 0 else 0.0

    print(f"\n=================================================================")
    print(f" 3-TIER RESOLVER POPULATION RESULTS ({total_tracks} TRACKS)")
    print(f"=================================================================")
    print(f" Tier 1 (mulib.db Ground Truth Transfer) : {t1_count:5d} tracks ({t1_count/total_tracks*100.0:.1f}%)")
    print(f" Tier 2 (AcousticBrainz Online API)      : {t2_count:5d} tracks ({t2_count/total_tracks*100.0:.1f}%)")
    print(f" Tier 3 (Local MusicNN ONNX Deep Net)    : {t3_count:5d} tracks ({t3_count/total_tracks*100.0:.1f}%)")
    print(f" Already Populated / Preserved           : {existing_count:5d} tracks ({existing_count/total_tracks*100.0:.1f}%)")
    print(f" Unresolved / Failed Tracks              : {fail_count:5d} tracks ({fail_count/total_tracks*100.0:.1f}%)")
    print(f"-----------------------------------------------------------------")
    print(f" TOTAL SUCCESS RATE                       : {success_rate:.2f}% ({total_successful}/{total_tracks})")
    print(f" TOTAL ELAPSED TIME                       : {elapsed:.2f} seconds ({elapsed/total_tracks:.3f} s/track)")
    print(f"=================================================================\n")

if __name__ == "__main__":
    main()
