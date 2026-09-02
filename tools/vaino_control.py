#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Reaching the co-resident player's own pages and process from inside
Sampo's console `[SPEC-SUI-140]`, `[SPEC-SUI-135]`.

Split out of `console.py`, where this cluster had grown directly against that
file's own stated safety claim: "the views, and nothing that writes... there
is no POST route in this file." `unflag_everywhere()` below is a write --
two, in fact, one local and one to vainopi -- and `console.py`'s own
`do_POST` has carried a route to it since `[REQ-VIS-265]`. The claim that
still holds, and the one this split makes structural again rather than
aspirational, is narrower: **this module never opens the library for
writing.** Every function here either asks the operating system a question
(is this port listening, is there a binary on `PATH`, open a terminal) or
signals the *already-running* Vaino process over HTTP/SSH to write its own
listener state -- the identical `POST /history/flag/:kind/:id` route the
player's own play-history page already calls (`player/src/web.rs`'s
`set_flag`). Nothing here executes a `sqlite3` write, or even opens one.

**Deliberately no `import console`.** `console.py` is run directly
(`python tools/console.py ...`), which loads it as `__main__` -- a module
that is *not* registered in `sys.modules` under the name `console`. A
top-level `import console` from here would therefore not reuse that running
process's module; it would load a second, independent copy of
`console.py`, with its own fresh `STATE` dict that main()'s argument
parsing never touches. Every value this module would otherwise have read
from `console.STATE` -- the open library's path, Sampo's own build info,
which subjects to clear and what the remote already thinks -- is instead a
parameter its caller passes in. `console.py` still owns that state and the
database reads that produce it; this module only ever receives the answer.
"""

import http.client
import json
import os
import shlex
import shutil
import socket
import subprocess
import sys
import time

# The player is 5720. A different number because they are different services
# on the same machine, and because `[SPEC-SUI-170]` may start the player:
# colliding would make each look like the other's failure.
VAINO_PORT = 5720


def _vaino_reachable(port: int, timeout: float = 0.5) -> bool:
    """A socket question, not a route question `[SPEC-SUI-170]`."""
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=timeout):
            return True
    except OSError:
        return False


def _vaino_has_sampo_support(port: int, timeout: float = 2.0) -> bool:
    """Whether *this* running Vaino was built with `--features sampo-support`
    `[SPEC-SUI-213]` -- the one thing `_vaino_reachable`'s socket question
    cannot tell apart: an appliance-equivalent build and a desktop build
    listen identically, and only one of them has anywhere for a handoff to
    land. `/review.js` is a static asset compiled in only by that feature
    `[SPEC-SUI-190]`, so its presence is a build-capability question, not a
    library one -- nothing about *this* library, or any library, is read
    here, which is the boundary `[SPEC-SUI-025]` actually protects. A
    real-world dead handoff (a Vaino running, answering, and 404ing every
    review link) is what this exists to catch before a person clicks it.
    """
    try:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
        try:
            conn.request("GET", "/review.js")
            r = conn.getresponse()
            r.read()  # drain -- the body is never inspected, only the status
            return r.status < 400
        finally:
            conn.close()
    except OSError:
        return False


def _vaino_set_flag(port: int, kind: str, subject_id: str, flagged: bool,
                     timeout: float = 2.0) -> bool:
    """Sampo signals; Vaino writes `[SPEC-SC-020]` -- the identical `POST
    /history/flag/:kind/:id` route the play-history page's own checkbox
    already calls (`player/src/web.rs`'s `set_flag`), never a direct
    `listener_flags` write from this process. `204` is the only success;
    anything else (a malformed kind, a closed connection, an older Vaino
    with no such route) is `False`, for the caller to report.
    """
    try:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
        try:
            conn.request("POST", f"/history/flag/{kind}/{subject_id}"
                                  f"?flagged={'true' if flagged else 'false'}")
            r = conn.getresponse()
            r.read()
            return r.status == 204
        finally:
            conn.close()
    except OSError:
        return False


def _remote_set_flag(remote: str, port: int, kind: str, subject_id: str, flagged: bool,
                      timeout: float = 8.0) -> bool:
    """The identical signal `_vaino_set_flag` sends locally, sent instead to
    vainopi's own already-running Vaino over one `ssh ... curl` round trip.
    Never a database write from this process, and never a service
    interruption: unlike `[SPEC-DF-111]`'s patch-apply recipe, nothing here
    writes to vainopi's database directly, so there is nothing for its own
    running player to race against and no reason to stop it.
    """
    host, _, _ = remote.partition(":")
    url = (f"http://localhost:{port}/history/flag/{kind}/{subject_id}"
           f"?flagged={'true' if flagged else 'false'}")
    try:
        r = subprocess.run(
            ["ssh", "-o", "ConnectTimeout=5", "-o", "BatchMode=yes", host,
             f"curl -s -o /dev/null -w '%{{http_code}}' --max-time 5 -X POST {shlex.quote(url)}"],
            capture_output=True, text=True, timeout=timeout)
    except (subprocess.TimeoutExpired, OSError):
        return False
    return r.returncode == 0 and r.stdout.strip() == "204"


def unflag_everywhere(subjects: list, remote: str | None, status: dict | None,
                       port: int = VAINO_PORT) -> dict:
    """Clear every plausible flag on this passage, locally and on the
    remote, in one action `[REQ-VIS-265]`. Both writes go through Vaino's
    own `set_flag` -- co-resident over plain HTTP, vainopi's over one `ssh
    ... curl` round trip -- never a `listener_flags` write from this
    process: listener state is Vaino's to write, not Sampo's.

    Takes what `console.py`'s own read-only queries already resolved,
    rather than a connection and a passage id: `subjects` is
    `passage_flag_subjects(conn, pid)` -- a `("passage", pid)` entry and
    one `("recording", mbid)` entry per linked recording. `status` is
    `flag_sync_status(conn, pid)`, or `None` when no remote is configured at
    all; when given, its `remote_pid`/`remote_mbids` are what let a
    `("passage", pid)` subject -- meaningful only locally, since `pid` is
    not portable `[SPEC-DF-103]` -- be translated to the remote's OWN local
    passage_id before being sent there, and let a `("recording", mbid)`
    subject be unioned with whatever the remote *itself* currently links.

    That union matters: found live the first time this ran for real, an id
    correction accepted locally but not yet pushed left the remote still
    linked to the *old* recording, so resolving subjects only from this
    library's own current link cleared nothing where the flag actually was,
    `_remote_set_flag` reporting success regardless (a DELETE matching zero
    rows is not an error). The same reasoning `[SPEC-DF-112]`'s
    `clear_flags_for()` already applies for an `id_review`'s own
    target+baseline, generalized: ask what the far side currently thinks
    too, clear the union. A remote passage that does not exist there at all
    (`status["remote_pid"] is None`) has its passage-keyed subject skipped,
    not silently dropped -- it is simply not in the union to begin with.
    """
    local_ok = [_vaino_set_flag(port, kind, sid, False) for kind, sid in subjects]
    result = {"local": {"ok": all(local_ok), "cleared": sum(local_ok), "of": len(local_ok)}}

    if not remote:
        result["remote"] = {"configured": False}
        return result
    if status is None or not status["reachable"]:
        result["remote"] = {"configured": True, "reachable": False}
        return result

    remote_subjects = {("recording", mbid) for mbid in status["remote_mbids"]}
    remote_subjects |= {(k, sid) for k, sid in subjects if k == "recording"}
    if status["remote_pid"] is not None:
        remote_subjects.add(("passage", str(status["remote_pid"])))
    remote_ok = [_remote_set_flag(remote, port, kind, sid, False)
                 for kind, sid in sorted(remote_subjects)]
    result["remote"] = {"configured": True, "reachable": True, "ok": all(remote_ok),
                         "cleared": sum(remote_ok), "of": len(remote_ok)}
    return result


def _vaino_binary() -> str | None:
    """Where the co-resident player's binary is, if one can be found at all.

    Checked against this repository's own build layout first -- the case
    while Sampo and Vaino are developed side by side -- then `PATH`, for an
    installed player. Never guessed beyond that: a wrong binary started
    against the wrong database is worse than admitting there is none.
    """
    here = os.path.dirname(os.path.abspath(__file__))
    for rel in (
        os.path.join(here, "..", "player", "target", "release", "vaino.exe"),
        os.path.join(here, "..", "player", "target", "release", "vaino"),
    ):
        if os.path.isfile(rel):
            return os.path.abspath(rel)
    return shutil.which("vaino")


def _vaino_build(port: int, timeout: float = 2.0) -> dict | None:
    """This co-resident Vaino's own build identity `[SPEC-SUI-227]` -- `GET
    /build`, the machine-readable sibling of the Settings page's "Server
    build" row `player/src/web.rs`'s `build_identity` serves. `None` on
    anything that stops this from being read -- an older Vaino with no such
    route, a non-JSON body, a closed connection -- and the caller treats an
    unknown build the same honest way `console.build_info()` already treats
    a missing git checkout: silently skipping a check it cannot make, never
    guessing `[SPEC-DF-095]`.
    """
    try:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=timeout)
        try:
            conn.request("GET", "/build")
            r = conn.getresponse()
            body = r.read()
            if r.status >= 400:
                return None
            return json.loads(body)
        finally:
            conn.close()
    except (OSError, ValueError):
        return None


def _vaino_staleness(port: int, sampo_build: dict | None) -> str | None:
    """Whether the co-resident Vaino was built from a different commit than
    *this* Sampo is running from `[SPEC-SUI-227]` -- found live 2026-08-31: a
    Vaino that was the only one running, and was sampo-support-capable, was
    still hours behind the checkout, so a boundary edit saved through it
    silently failed to reappear on reopening the editor -- the merge-with-
    draft logic that fixed did not exist in that build. Neither
    `[SPEC-SUI-170]`'s reuse check nor `[SPEC-SUI-213]`'s capability probe
    would have caught that; both ask "is something there," never "is it the
    same something this checkout would build."

    `sampo_build` is `console.STATE["build"]` -- this Sampo's own
    `build_info()` result, computed once at startup. `None` -- no mismatch,
    or nothing to compare -- whenever either side's identity is unknown: no
    git checkout, git missing from `PATH`, or a Vaino too old to serve
    `/build` at all. A silent skip, not a guess, matching `build_info()`'s
    own posture toward an absent checkout.
    """
    sampo = sampo_build or {}
    sampo_commit = sampo.get("commit") if sampo.get("available") else None
    if not sampo_commit:
        return None
    vaino = _vaino_build(port)
    vaino_git = (vaino or {}).get("git")
    if not vaino_git or vaino_git == "unknown":
        return None
    vaino_hash = vaino_git.removesuffix("+dirty")
    if sampo_commit.startswith(vaino_hash):
        return None
    return (f"Sampo is running from commit {sampo.get('commit_short') or sampo_commit[:12]}, "
            f"but the co-resident Vaino on this port was built from a different commit "
            f"({vaino_git}, {vaino.get('commit_date', 'date unknown')}) -- "
            "rebuild whichever one is behind (see HOWTO.md §2) and restart it")


def _vaino_ready(port: int, started: bool, sampo_build: dict | None = None) -> dict:
    """A reachable Vaino is not necessarily a *useful* one for this handoff
    `[SPEC-SUI-213]` -- found live several times over on 2026-08-30: a plain
    appliance-equivalent build answers every socket check `ensure_vaino()`
    could make and still 404s every review/edit link, which read in a
    browser as a dead page with no explanation. Named here instead, the same
    "say which capability is unavailable, and why" `[SPEC-SUI-170]` already
    commits to for a missing binary or a start that timed out.
    """
    if not _vaino_has_sampo_support(port):
        binary = _vaino_binary()
        return {"ok": False, "port": port, "started": started,
                "error": ("Sampo just started a local Vaino, but " if started else
                          "a Vaino is already running on this port, but ")
                         + (f"{binary} " if binary else "the binary ")
                         + "was built without --features sampo-support, so the review page "
                           "and waveform editor don't exist in it (see HOWTO.md §2). "
                           "Rebuild player/ with that flag, then " +
                           ("restart it" if started else "stop this one and reopen this page")}
    stale = _vaino_staleness(port, sampo_build)
    if stale:
        return {"ok": False, "port": port, "started": started, "error": stale}
    return {"ok": True, "port": port, "started": started}


def ensure_vaino(port: int = VAINO_PORT, db_path: str | None = None,
                  sampo_build: dict | None = None) -> dict:
    """Start the co-resident player if one is not already there `[SPEC-SUI-170]`.

    `db_path` is `console.STATE["path"]` -- the library Sampo has open --
    and `sampo_build` is `console.STATE["build"]`, threaded through to
    `_vaino_ready` for the staleness check.

    1. **Already running?** Use it. Do not start a second -- two players on
       one library contend for the audio device and both write the single
       resume row `[SPEC-SC-098]`.
    2. **Not running?** Start it, on **Sampo's own database path**. This is
       what makes `[SPEC-SUI-150]`'s passage-id handoff sound: the player
       reads the exact file the id came from because Sampo told it to, not
       because a configuration happened to agree.
    3. **Start failed, or started without the routes this handoff needs?**
       Say which capability is unavailable, and why. Silent degradation is
       its own failure `[SPEC-DF-095]`.
    """
    if _vaino_reachable(port):
        return _vaino_ready(port, started=False, sampo_build=sampo_build)

    if not db_path:
        return {"ok": False, "port": port, "error": "no library open"}

    binary = _vaino_binary()
    if not binary:
        return {"ok": False, "port": port,
                "error": "no local Vaino binary found -- build player/ first "
                         "(see build/README.md)"}

    try:
        subprocess.Popen(
            [binary, db_path, "--port", str(port)],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
    except OSError as e:
        return {"ok": False, "port": port, "error": f"could not start vaino: {e}"}

    # Polled, not a fixed wait: the Program Director's own startup time scales
    # with library size, the same reason `deploy-player.sh` polls rather than
    # sleeping a fixed span before declaring a new build alive.
    deadline = time.time() + 20
    while time.time() < deadline:
        if _vaino_reachable(port):
            return _vaino_ready(port, started=True, sampo_build=sampo_build)
        time.sleep(0.25)
    return {"ok": False, "port": port,
            "error": "vaino did not answer within 20s of starting"}


def open_terminal(directory: str) -> dict:
    """Open an ordinary terminal on THIS machine, in `directory` `[IMPL007
    Stage 5]`.

    Not SSH, not rsync, no remote host known to this process at all -- opening
    a local terminal is the same act as the operator opening one themselves,
    just one click closer to the deploy commands the page already prints as
    plain, selectable text. The commands are never typed for them and never
    run by this process; the window is a place to paste them, or type them by
    hand, and see exactly what runs before it does.
    """
    if not os.path.isdir(directory):
        return {"ok": False, "error": f"no such directory: {directory}"}
    try:
        if sys.platform == "win32":
            subprocess.Popen(["cmd", "/K", f'cd /d "{directory}"'],
                             creationflags=subprocess.CREATE_NEW_CONSOLE)
        elif sys.platform == "darwin":
            subprocess.Popen(["open", "-a", "Terminal", directory])
        else:
            # Best effort across desktops -- there is no single "the terminal"
            # on Linux the way there is on the other two platforms.
            for candidate in ("x-terminal-emulator", "gnome-terminal", "konsole", "xterm"):
                if shutil.which(candidate):
                    subprocess.Popen([candidate], cwd=directory)
                    break
            else:
                return {"ok": False,
                        "error": "no terminal emulator found on PATH "
                                 "(tried x-terminal-emulator, gnome-terminal, konsole, xterm)"}
        return {"ok": True}
    except OSError as e:
        return {"ok": False, "error": f"could not open a terminal: {e}"}
