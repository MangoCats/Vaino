#!/bin/bash
# Rebuild and redeploy the player everywhere it runs: this desktop, vainopi,
# and bose. Written after the targets were left to drift more than once --
# code committed and pushed, but only actually running on some of them,
# discovered later by Vaino's own staleness check firing rather than by
# anyone remembering to look [SPEC-SUI-227].
#
#     build/deploy-everywhere.sh                        # local, vainopi, bose
#     build/deploy-everywhere.sh -- pi@other-host       # one named appliance
#     build/deploy-everywhere.sh -- pi@one pi@two ...   # several
#
# bose was added after doing exactly what this script exists to prevent: it
# sat four commits behind while vainopi was kept current, and nobody noticed
# until someone asked. It runs the SAME binary -- `[BOS-RUN-010]` verified
# that one aarch64 build serves both appliances, because the cross-build
# image is bookworm and the binary imports nothing above GLIBC_2.34, which
# both vainopi (2.36) and bose (2.41) satisfy.
#
# Every leg runs regardless of whether the others succeeded -- a broken local
# build is not a reason to leave an appliance on stale code, or the reverse --
# and the exit status is the number that failed, the same accumulate-then-
# report shape build/verify-targets.sh already uses across its own targets.
#
# Each appliance leg runs the full build-and-deploy rather than building once
# and pushing the artefact twice. The second cross-compile is very nearly
# free -- docker's image is cached and cargo finds nothing to redo -- and in
# exchange every leg independently guarantees it shipped a binary built from
# the tree as it stands, instead of one leg trusting an artefact another leg
# was supposed to have produced.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
HOSTS="pi@vainopi pi@bose"
if [ "${1:-}" = "--" ]; then
    shift
    # Everything after `--` is the host list, so naming one appliance means
    # that one alone rather than that one plus the defaults. A bare `--` with
    # nothing after it keeps them, rather than deploying to nowhere and
    # reporting success for having done so.
    if [ "$#" -gt 0 ]; then
        HOSTS="$*"
    fi
fi

fail=0

echo "== local =="
"$ROOT/build/deploy-local.sh" || fail=$((fail + 1))

for host in $HOSTS; do
    echo
    echo "== ${host#*@} ($host) =="
    "$ROOT/build/deploy-vainopi.sh" "$host" || fail=$((fail + 1))
done

# A final, authoritative check against HEAD *right now* -- not each leg's own
# earlier-in-time verification. This is the only thing that can catch a
# commit landing between the legs above, or between this script starting
# and finishing; it CANNOT catch one landing after this script exits, which
# is exactly what happened the first time this script existed: it deployed
# `fe4f07d` correctly, and the very next commit (this file's own predecessor)
# moved HEAD again with nobody redeploying afterward. There is no script fix
# for that -- only a rule: run this LAST, after every commit in a change is
# already made, never before one more is still coming.
echo
echo "== verifying every target matches HEAD =="
head_sha=$(cd "$ROOT" && git rev-parse --short HEAD 2>/dev/null || echo "unknown")
port="${VAINO_PORT:-5720}"
local_build=$(curl -s --max-time 3 "http://localhost:$port/build" 2>/dev/null)

# Width of the widest label, so the report lines up however the hosts are
# named -- `pi@vainopi` and `pi@bose` are not the same length.
width=5
for host in $HOSTS; do
    name=${host#*@}
    [ "${#name}" -gt "$width" ] && width=${#name}
done

check_matches() {
    label="$1"; json="$2"
    case "$json" in
        *"$head_sha"*) printf '  %-*s : matches HEAD (%s)
' "$width" "$label" "$head_sha"; return 0 ;;
        *) printf '  %-*s : does NOT match HEAD (%s) -- got: %s
'                "$width" "$label" "$head_sha" "${json:-no answer}" >&2; return 1 ;;
    esac
}
mismatch=0
check_matches "local" "$local_build" || mismatch=$((mismatch + 1))
for host in $HOSTS; do
    remote_build=$(ssh -o ConnectTimeout=5 "$host" "curl -s --max-time 3 http://localhost:$port/build" 2>/dev/null)
    check_matches "${host#*@}" "$remote_build" || mismatch=$((mismatch + 1))
done
fail=$((fail + mismatch))

echo
if [ "$fail" -eq 0 ]; then
    echo "deploy-everywhere: every target is current, matching HEAD ($head_sha)"
else
    echo "deploy-everywhere: $fail target(s) failed or do not match HEAD -- see above" >&2
    [ "$mismatch" -gt 0 ] && echo "(a commit landing mid-run is the usual cause -- re-run this script)" >&2
fi
exit "$fail"
