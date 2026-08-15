import sqlite3
import sys

sys.stdout.reconfigure(encoding='utf-8')

VAINO_DB = r"C:\Users\Mango Cat\Dev\Vaino\vaino.db"

AB_COLS = [
    "ab_acoustic",
    "ab_aggressive",
    "ab_bright",
    "ab_danceable",
    "ab_female",
    "ab_happy",
    "ab_instrumental",
    "ab_party",
    "ab_relaxed",
    "ab_sad",
    "ab_tonal"
]

def main():
    conn = sqlite3.connect(VAINO_DB)
    conn.row_factory = sqlite3.Row
    cur = conn.execute("SELECT * FROM tracks")
    tracks = cur.fetchall()
    total_tracks = len(tracks)

    print(f"=================================================================")
    print(f" EVALUATING 11D ACOUSTIC FEATURE PRESENCE ({total_tracks} TRACKS)")
    print(f" Target Database: {VAINO_DB}")
    print(f"=================================================================\n")

    missing_counts = {col: 0 for col in AB_COLS}
    present_counts = {col: 0 for col in AB_COLS}
    tracks_missing_any = 0
    tracks_missing_all = 0
    tracks_fully_populated = 0

    for r in tracks:
        d = dict(r)
        missing_here = 0
        for col in AB_COLS:
            val = d.get(col)
            if val is None:
                missing_counts[col] += 1
                missing_here += 1
            else:
                present_counts[col] += 1

        if missing_here == 0:
            tracks_fully_populated += 1
        elif missing_here == len(AB_COLS):
            tracks_missing_all += 1
        else:
            tracks_missing_any += 1

    print(f"FEATURE BREAKDOWN:")
    print(f"-----------------------------------------------------------------")
    print(f"{'Feature Name':<18} | {'Present':<12} | {'Missing (Lacking)':<18} | {'Missing %':<10}")
    print(f"-----------------------------------------------------------------")
    for col in AB_COLS:
        p_cnt = present_counts[col]
        m_cnt = missing_counts[col]
        m_pct = (m_cnt / total_tracks) * 100.0
        print(f"{col:<18} | {p_cnt:6d} ({p_cnt/total_tracks*100.0:5.1f}%) | {m_cnt:6d} tracks      | {m_pct:6.2f}%")
    print(f"-----------------------------------------------------------------\n")

    print(f"SUMMARY OF TRACK COVERAGE:")
    print(f"-----------------------------------------------------------------")
    print(f" Total Library Tracks             : {total_tracks:5d} tracks")
    print(f" Tracks with ALL 11 Features      : {tracks_fully_populated:5d} tracks ({tracks_fully_populated/total_tracks*100.0:.2f}%)")
    print(f" Tracks Missing 1+ Features       : {tracks_missing_any:5d} tracks ({tracks_missing_any/total_tracks*100.0:.2f}%)")
    print(f" Tracks Missing ALL 11 Features   : {tracks_missing_all:5d} tracks ({tracks_missing_all/total_tracks*100.0:.2f}%)")
    print(f"=================================================================\n")

    conn.close()

if __name__ == "__main__":
    main()
