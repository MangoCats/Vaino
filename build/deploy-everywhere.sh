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

echo
if [ "$fail" -eq 0 ]; then
    echo "deploy-everywhere: both targets are current"
else
    echo "deploy-everywhere: $fail target(s) failed -- see above"
fi
exit "$fail"
