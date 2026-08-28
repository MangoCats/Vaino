#!/bin/bash
# Build Vaino for the appliance and put it there, in one command.
#
#   VainoPi/deploy.sh                                  latest: whatever is checked out here
#   VainoPi/deploy.sh pi-audio-stable-2026-08-16        a tag, without touching your own checkout
#   VainoPi/deploy.sh pi-audio-stable-2026-08-16 pi@x   a tag, to a different host
#   VainoPi/deploy.sh pi@x                              latest, to a different host
#
# Wraps the two steps this used to mean doing by hand: cross-compiling
# (build/README.md) and VainoPi/deploy-player.sh. Ends by asking the
# appliance which commit it is actually running and refusing to call it
# done if the answer disagrees -- the same reason deploy-player.sh already
# asks the running process to identify itself rather than trusting a
# checksum alone.
set -uo pipefail

# Git Bash on Windows "helpfully" rewrites any argument that looks like a
# bare Unix path -- including a container-side path such as `/w` or
# `/build.sh`, which have nothing to do with the host filesystem at all --
# into a Windows path. Inside a `-v` flag this only fires when the host-side
# half is ALSO a bare Unix path (`/tmp/...`); once the host half is already a
# real Windows path (`C:/...`, what `win_path` below produces), the
# container-side half of that flag is left alone. So every mount source in
# this script is put through `win_path` first. A bare container-only path
# used elsewhere on a command line -- e.g. `bash /build.sh` as the container
# command below -- still gets rewritten regardless, since MSYS does not know
# it is not this command's `-v` flag; those spots need `MSYS_NO_PATHCONV`
# scoped to just that one command (`VAR=val cmd`, not exported), since
# exporting it globally would also block the POSIX-style paths *this
# script's own* `git`/`pwd` calls use from becoming ones `git.exe` can open.

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
REF="${1:-}"
HOST="${2:-pi@vainopi}"
# A lone argument that looks like `user@host` is a host, not a ref -- so
# "deploy latest, elsewhere" doesn't need the awkward `deploy.sh "" pi@x`.
case "$REF" in *@*) HOST="$REF"; REF="" ;; esac

die() { echo "deploy: $*" >&2; exit 1; }

docker info >/dev/null 2>&1 \
    || die "Docker is not running -- start Docker Desktop (or dockerd) and try again"

# Git Bash on Windows needs the native path for Docker's bind mount; `pwd`
# alone gives the POSIX-style one Docker Desktop mismounts
# [build/README.md]. Plain Linux/macOS has no `-W` flag, hence the fallback.
win_path() { (cd "$1" && pwd -W 2>/dev/null) || echo "$1"; }
DOCKER_PATH="$(win_path "$REPO_ROOT")"

echo "deploy: building the aarch64 cross-compiler image (cached after the first run)"
docker build -t vaino-aarch64 -f "$DOCKER_PATH/build/Dockerfile.aarch64" "$DOCKER_PATH" \
    || die "docker build failed"

OUT="$REPO_ROOT/player/target/aarch64-unknown-linux-gnu/release/vaino"

if [ -z "$REF" ]; then
    # No ref named: build exactly what is checked out here, same as running
    # the two steps by hand always has. A dirty tree is refused rather than
    # silently stamped: the appliance's own self-report exists so a deploy
    # can be trusted later, and a build from edited sources is not the
    # commit it would otherwise claim to be [player/build.rs].
    if [ -n "$(git -C "$REPO_ROOT" status --porcelain)" ] && [ "${ALLOW_DIRTY:-}" != "1" ]; then
        die "uncommitted changes are present -- commit first, or re-run with ALLOW_DIRTY=1 to build this exact tree anyway"
    fi
    EXPECTED="$(git -C "$REPO_ROOT" rev-parse --short=12 HEAD)"
    git -C "$REPO_ROOT" diff --quiet --ignore-cr-at-eol HEAD 2>/dev/null || EXPECTED="${EXPECTED}+dirty"

    echo "deploy: cross-compiling the currently checked-out tree ($EXPECTED)..."
    docker run --rm -v "$DOCKER_PATH:/w" vaino-aarch64 \
        cargo build --release --target aarch64-unknown-linux-gnu --manifest-path player/Cargo.toml \
        || die "cross-compile failed"
else
    # A named ref: built in a worktree that is created and used entirely
    # inside the container, so your own checkout is never touched and a
    # worktree's own gitdir pointer never has to resolve across the
    # host/container boundary at all -- the same class of path-translation
    # fault the mount above already has to work around.
    git -C "$REPO_ROOT" rev-parse --verify "$REF" >/dev/null 2>&1 \
        || die "no such ref: $REF (try 'git tag' or 'git log --oneline' to see what exists)"
    # `^{commit}` peels an annotated tag to what it actually points at --
    # without it, an annotated tag's OWN object hash gets compared against
    # the commit hash the checkout (correctly) lands on, and every annotated
    # tag would look like a mismatch even though the right thing was built.
    EXPECTED="$(git -C "$REPO_ROOT" rev-parse --short=12 "$REF^{commit}")"

    echo "deploy: cross-compiling $REF ($EXPECTED) in its own worktree..."
    # Written inside the repo itself (not `mktemp`, which on Git Bash lands
    # in MSYS's own virtual /tmp -- a path docker.exe cannot resolve at all,
    # so it silently mounts an empty directory at /build.sh instead of the
    # script) so that `win_path` can turn it into the real Windows path the
    # mount needs, the same way it already does for the repo checkout below.
    BUILD_SCRIPT="$REPO_ROOT/.vaino-deploy-build.sh"
    trap 'rm -f "$BUILD_SCRIPT"' EXIT
    cat > "$BUILD_SCRIPT" <<'EOS'
set -e
cd /w
git worktree add --detach --force /tmp/vaino-build "$1" >/dev/null
cd /tmp/vaino-build
cargo build --release --target aarch64-unknown-linux-gnu --manifest-path player/Cargo.toml
mkdir -p /w/.deploy-out
cp player/target/aarch64-unknown-linux-gnu/release/vaino /w/.deploy-out/vaino
cd /w
git worktree remove --force /tmp/vaino-build
EOS
    # `MSYS_NO_PATHCONV` scoped to this one command: both mount sources above
    # are already Windows-style paths, so nothing here needs the POSIX-to-
    # Windows conversion MSYS normally does -- but MSYS applies that same
    # conversion to a bare-looking argument anywhere in the command line, not
    # just inside `-v` flags, and would otherwise turn the trailing `bash
    # /build.sh` (a path that only exists inside the container) into a
    # nonexistent path on the host's own filesystem.
    MSYS_NO_PATHCONV=1 docker run --rm \
        -v "$DOCKER_PATH:/w" -v "$DOCKER_PATH/.vaino-deploy-build.sh:/build.sh:ro" vaino-aarch64 \
        bash /build.sh "$REF" \
        || die "cross-compile failed"
    mkdir -p "$(dirname "$OUT")"
    mv "$REPO_ROOT/.deploy-out/vaino" "$OUT"
    rmdir "$REPO_ROOT/.deploy-out" 2>/dev/null || true
fi

echo "deploy: putting it on $HOST"
# deploy-player.sh looks for the binary at a path relative to wherever it is
# run from, not relative to itself -- so this only works run from anywhere
# (including from inside VainoPi/ itself) because of the `cd` here.
(cd "$REPO_ROOT" && "$HERE/deploy-player.sh" "$HOST") || die "deploy-player.sh failed; see above"

# The checksum-and-restart above proves *a* new binary answers; this proves
# it is the *right* one, by asking the same way a person checking by hand
# would [SPEC-APS-140].
REPORTED="$(ssh "$HOST" "/usr/local/bin/vaino --version" 2>/dev/null \
            | grep -o '[0-9a-f]\{12\}\(+dirty\)\?')"
if [ "$REPORTED" = "$EXPECTED" ]; then
    echo "deploy: confirmed -- $HOST is running $EXPECTED"
elif [ -z "$REPORTED" ]; then
    # `--version` itself was added in 421f7c1 [REQ-VIS-200]; a ref older than
    # that has nothing to ask. deploy-player.sh already confirmed the upload
    # checksum and a live restart, so the deploy is not in question -- only
    # this last, stricter cross-check is unavailable for a build this old.
    echo "deploy: $HOST is running the new build (checksum-verified), but it predates --version [421f7c1] and cannot self-report which commit that is"
else
    die "$HOST reports '$REPORTED', expected '$EXPECTED' -- something is not what it seems"
fi
