import os
import sys
import random
import sqlite3
import numpy as np
import logging

# Ensure project root is in sys.path
sys.path.insert(0, r"C:\Users\Mango Cat\Dev\Vaino")

from src.db.acoustic_resolver import AcousticResolver, AB_COLS

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger(__name__)

MULIB_PATH = r"C:\Users\Mango Cat\Dev\MuLibPlay\mulib.db"
MUSIC_DIR = r"C:\Users\Mango Cat\Music"

def main():
    if not os.path.exists(MULIB_PATH):
        print(f"Error: mulib.db not found at {MULIB_PATH}")
        return

    conn = sqlite3.connect(MULIB_PATH)
    conn.row_factory = sqlite3.Row
    cur = conn.execute("""
        SELECT f.filePath, t.abAcoustic, t.abAggressive, t.abBright, t.abDanceable, t.abFemale, t.abHappy,
               t.abInstrumental, t.abParty, t.abRelaxed, t.abSad, t.abTonal
        FROM tracks t
        JOIN cuts c ON c.trackId = t.trackId
        JOIN files f ON f.fileId = c.fileId
        WHERE t.abAcoustic IS NOT NULL AND f.filePath IS NOT NULL
    """)
    all_rows = cur.fetchall()
    conn.close()

    print(f"Found {len(all_rows)} total tracks in mulib.db with Tier 1 AcousticBrainz data.")

    # Match paths to physical files on disk under C:\Users\Mango Cat\Music
    matched_tracks = []
    for r in all_rows:
        raw_path = r["filePath"]
        # Convert path delimiters and extract relative tail (e.g. 'Artist/Album/Track.mp3')
        norm_path = raw_path.replace('/', '\\')
        parts = norm_path.split('\\')
        
        # Look for matching file under MUSIC_DIR
        actual_path = None
        if len(parts) >= 3:
            tail_3 = os.path.join(MUSIC_DIR, *parts[-3:])
            if os.path.exists(tail_3):
                actual_path = tail_3
        if not actual_path and len(parts) >= 2:
            tail_2 = os.path.join(MUSIC_DIR, *parts[-2:])
            if os.path.exists(tail_2):
                actual_path = tail_2

        if actual_path and os.path.exists(actual_path):
            tier1_vals = [
                float(r["abAcoustic"]), float(r["abAggressive"]), float(r["abBright"]),
                float(r["abDanceable"]), float(r["abFemale"]), float(r["abHappy"]),
                float(r["abInstrumental"]), float(r["abParty"]), float(r["abRelaxed"]),
                float(r["abSad"]), float(r["abTonal"])
            ]
            matched_tracks.append((actual_path, tier1_vals))

    print(f"Successfully matched {len(matched_tracks)} tracks to physical audio files on disk.")

    if len(matched_tracks) == 0:
        print("No physical files matched! Checking directory structure...")
        return

    # Sample 100 random tracks
    random.seed(42)
    sample_size = min(100, len(matched_tracks))
    sampled = random.sample(matched_tracks, sample_size)

    print(f"\n=================================================================")
    print(f" RUNNING TIER 3 ONNX INFERENCE BENCHMARK vs TIER 1 ON {sample_size} TRACKS")
    print(f"=================================================================\n")

    tier1_matrix = []
    tier3_matrix = []
    processed_count = 0

    for idx, (fpath, t1_vals) in enumerate(sampled, 1):
        filename = os.path.basename(fpath)
        print(f"[{idx:3d}/{sample_size}] Processing: {filename[:45]:<45} ... ", end="", flush=True)

        res_tier3 = AcousticResolver._try_tier3_onnx(fpath)
        if res_tier3:
            t3_vals = [res_tier3[c] for c in AB_COLS]
            tier1_matrix.append(t1_vals)
            tier3_matrix.append(t3_vals)
            processed_count += 1
            print("OK")
        else:
            print("FAILED")

    if processed_count == 0:
        print("No tracks successfully extracted via Tier 3!")
        return

    T1 = np.array(tier1_matrix) # Shape: (N, 11)
    T3 = np.array(tier3_matrix) # Shape: (N, 11)

    print(f"\n=================================================================")
    print(f" BENCHMARK RESULTS ({processed_count} TRACKS COMPLETED)")
    print(f"=================================================================\n")

    print(f"{'Feature':<18} | {'MAE':<8} | {'Pearson R':<10} | {'Quality Assessment':<20}")
    print("-" * 65)

    pearsons = []
    maes = []

    for i, col_name in enumerate(AB_COLS):
        clean_name = col_name.replace('ab_', '').capitalize()
        mae = float(np.mean(np.abs(T1[:, i] - T3[:, i])))
        r = float(np.corrcoef(T1[:, i], T3[:, i])[0, 1]) if np.std(T1[:, i]) > 1e-6 and np.std(T3[:, i]) > 1e-6 else 0.0
        
        status = "HIGH QUALITY (>=0.85)" if r >= 0.85 else ("GOOD (>=0.70)" if r >= 0.70 else "MODERATE")
        print(f"{clean_name:<18} | {mae:<8.4f} | {r:<10.4f} | {status:<20}")
        pearsons.append(r)
        maes.append(mae)

    avg_r = float(np.mean(pearsons))
    avg_mae = float(np.mean(maes))
    dist_11d = float(np.mean(np.sqrt(np.mean((T1 - T3)**2, axis=1))))

    print("-" * 65)
    print(f"{'OVERALL AVERAGE':<18} | {avg_mae:<8.4f} | {avg_r:<10.4f} | {'HIGH QUALITY (>=0.85)' if avg_r >= 0.85 else 'ACCEPTABLE'}")
    print(f"\nAverage 11D Distance Error: {dist_11d:.4f}")
    print("=================================================================\n")

if __name__ == "__main__":
    main()
