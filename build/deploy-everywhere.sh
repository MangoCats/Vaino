#!/bin/bash
# Rebuild and redeploy the player everywhere it runs: this desktop, and
# vainopi. Written after the two were left to drift more than once -- code
# committed and pushed, but only actually running on one of the two targets,
# discovered later by Vaino's own staleness check firing rather than by
# anyone remembering to look [SPEC-SUI-227].
#
#     build/deploy-everywhere.sh                  # local db, pi@vainopi
#     build/deploy-everywhere.sh -- pi@other-host  # a different appliance
#
# Both legs run regardless of whether the other succeeded -- a broken local
# build is not a reason to leave vainopi on stale code, or the reverse --
# and the exit status is the number that failed, the same accumulate-then-
# report shape build/verify-targets.sh already uses across its own targets.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOST="pi@vainopi"
if [ "${1:-}" = "--" ]; then
    HOST="${2:-pi@vainopi}"
fi

fail=0

echo "== local =="
"$ROOT/build/deploy-local.sh" || fail=$((fail + 1))

echo
echo "== vainopi ($HOST) =="
"$ROOT/build/deploy-vainopi.sh" "$HOST" || fail=$((fail + 1))

# A final, authoritative check against HEAD *right now* -- not each leg's own
# earlier-in-time verification. This is the only thing that can catch a
# commit landing between the two legs above, or between this script starting
# and finishing; it CANNOT catch one landing after this script exits, which
# is exactly what happened the first time this script existed: it deployed
# `fe4f07d` correctly, and the very next commit (this file's own predecessor)
# moved HEAD again with nobody redeploying afterward. There is no script fix
# for that -- only a rule: run this LAST, after every commit in a change is
# already made, never before one more is still coming.
echo
echo "== verifying both targets match HEAD =="
head_sha=$(cd "$ROOT" && git rev-parse --short HEAD 2>/dev/null || echo "unknown")
port="${VAINO_PORT:-5720}"
local_build=$(curl -s --max-time 3 "http://localhost:$port/build" 2>/dev/null)
remote_build=$(ssh -o ConnectTimeout=5 "$HOST" "curl -s --max-time 3 http://localhost:$port/build" 2>/dev/null)

check_matches() {
    label="$1"; json="$2"
    case "$json" in
        *"$head_sha"*) echo "  $label: matches HEAD ($head_sha)"; return 0 ;;
        *) echo "  $label: does NOT match HEAD ($head_sha) -- got: ${json:-no answer}" >&2; return 1 ;;
    esac
}
mismatch=0
check_matches "local  " "$local_build" || mismatch=$((mismatch + 1))
check_matches "vainopi" "$remote_build" || mismatch=$((mismatch + 1))
fail=$((fail + mismatch))

echo
if [ "$fail" -eq 0 ]; then
    echo "deploy-everywhere: both targets are current, matching HEAD ($head_sha)"
else
    echo "deploy-everywhere: $fail target(s) failed or do not match HEAD -- see above" >&2
    [ "$mismatch" -gt 0 ] && echo "(a commit landing mid-run is the usual cause -- re-run this script)" >&2
fi
exit "$fail"
