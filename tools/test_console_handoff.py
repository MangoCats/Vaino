#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the Sampo→Vaino handoff in `vaino_control.py` [SPEC-SUI-170],
[SPEC-SUI-213], [SPEC-SUI-227].

Exercises the contract on its own -- liveness is a socket question, not a
route question; a missing library or a missing binary is reported, not
guessed past; an already-reachable port is used, never duplicated; per
`[SPEC-SUI-213]`, a *reachable* port whose Vaino was built without
`--features sampo-support` is named as such rather than left to 404 on the
next click; and, per `[SPEC-SUI-227]`, a capable port whose Vaino was built
from a *different commit* than this Sampo is named as stale, while an
unknowable comparison (no `/build` route, no git checkout under Sampo
itself) is silently skipped rather than guessed at -- all without needing a
real Vaino binary on the machine running the test. A minimal fake HTTP
server stands in for the builds that matter: one that serves `/review.js`,
one that 404s it, and a `/build` body set per test.

`db_path`/`sampo_build` are passed to `ensure_vaino()` directly rather than
through `console.STATE`, per the console.py/vaino_control.py split:
`vaino_control.py` takes its inputs as parameters, never a global, so there
is nothing here to save and restore around a test.

    python tools/test_console_handoff.py
"""

import http.server
import json
import os
import socket
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import vaino_control  # noqa: E402

FAILED = []


def check(cond, msg):
    if not cond:
        FAILED.append(msg)
        print(f"  FAIL: {msg}")


def free_port() -> int:
    """A port nothing is listening on, without a race: bind then release."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class _FakeVaino(http.server.BaseHTTPRequestHandler):
    """Answers every path 200 except `/review.js`, whose status is set per
    instance -- the one difference between a sampo-support build and a plain
    one that matters to `_vaino_has_sampo_support`. `/build` answers with
    `build_json` when set, or 404s -- standing in for a Vaino too old to have
    the route at all, which `_vaino_staleness` must treat as "unknown," not
    "mismatched" `[SPEC-SUI-227]`.
    """
    review_status = 200
    build_json = None

    def log_message(self, fmt, *args):
        pass  # quiet -- a passing test has nothing to say

    def do_GET(self):
        if self.path == "/build":
            if self.build_json is None:
                self.send_response(404)
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            body = json.dumps(self.build_json).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        status = self.review_status if self.path == "/review.js" else 200
        self.send_response(status)
        self.send_header("Content-Length", "0")
        self.end_headers()


def fake_vaino(review_status: int = 200, build_json: dict | None = None) -> http.server.HTTPServer:
    handler = type("Handler", (_FakeVaino,), {"review_status": review_status, "build_json": build_json})
    srv = http.server.HTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


def main() -> int:
    print("liveness is a socket question, not a route question")
    port = free_port()
    check(not vaino_control._vaino_reachable(port, timeout=0.2),
          "a port nothing is listening on must read as unreachable")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        bound = srv.getsockname()[1]
        check(vaino_control._vaino_reachable(bound, timeout=0.5),
              "a port something is listening on must read as reachable")

    print()
    print("already running, and it has the routes this handoff needs: used, not duplicated")
    srv = fake_vaino(review_status=200)
    try:
        port = srv.server_port
        result = vaino_control.ensure_vaino(port=port)
        check(result == {"ok": True, "port": port, "started": False},
              f"an already-reachable, capable port must be reported as such, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("already running, but built without sampo-support: named as the missing "
          "capability, not left to 404 on the next click [SPEC-SUI-213]")
    srv = fake_vaino(review_status=404)
    try:
        port = srv.server_port
        result = vaino_control.ensure_vaino(port=port)
        check(result["ok"] is False, f"expected a refusal, got {result}")
        check(result.get("started") is False, f"this Vaino was already running, got {result}")
        err = result.get("error", "")
        check("sampo-support" in err, f"the reason must name the missing feature, got {result}")
        check("HOWTO.md" in err, f"the reason must point somewhere to fix it, got {result}")
        check("already running" in err, f"must say this one was found, not started, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("the wording differs when Sampo itself just started the incapable binary "
          "(no real subprocess needed -- _vaino_ready() takes `started` directly)")
    srv = fake_vaino(review_status=404)
    try:
        result = vaino_control._vaino_ready(srv.server_port, started=True)
        check(result["ok"] is False and result["started"] is True, f"got {result}")
        check("Sampo just started" in result.get("error", ""), f"got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("same commit as this Sampo: capable and current, no staleness reported "
          "[SPEC-SUI-227]")
    sampo_build = {"available": True, "commit": "a" * 40, "commit_short": "aaaaaaaaaaaa"}
    srv = fake_vaino(build_json={"git": "a" * 12, "branch": "main", "commit_date": "2026-08-31"})
    try:
        result = vaino_control.ensure_vaino(port=srv.server_port, sampo_build=sampo_build)
        check(result == {"ok": True, "port": srv.server_port, "started": False},
              f"a matching commit must not be reported as stale, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("different commit than this Sampo: named as stale, not silently served "
          "[SPEC-SUI-227]")
    sampo_build = {"available": True, "commit": "a" * 40, "commit_short": "aaaaaaaaaaaa"}
    srv = fake_vaino(build_json={"git": "b" * 12, "branch": "main", "commit_date": "2026-08-30"})
    try:
        result = vaino_control.ensure_vaino(port=srv.server_port, sampo_build=sampo_build)
        check(result["ok"] is False, f"expected a refusal, got {result}")
        err = result.get("error", "")
        check("different commit" in err, f"the reason must name the mismatch, got {result}")
        check("bbbbbbbbbbbb" in err, f"the reason must name the Vaino's own commit, got {result}")
        check("aaaaaaaaaaaa" in err, f"the reason must name Sampo's own commit, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("a dirty build's hash still compares against the bare commit -- the "
          "+dirty suffix names the working tree, not a different commit "
          "[SPEC-SUI-227]")
    sampo_build = {"available": True, "commit": "a" * 40, "commit_short": "aaaaaaaaaaaa"}
    srv = fake_vaino(build_json={"git": ("a" * 12) + "+dirty", "branch": "main", "commit_date": "2026-08-31"})
    try:
        result = vaino_control.ensure_vaino(port=srv.server_port, sampo_build=sampo_build)
        check(result == {"ok": True, "port": srv.server_port, "started": False},
              f"a dirty build of the same commit must not be reported as stale, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("no /build route at all (an older Vaino): unknowable, so skipped rather "
          "than guessed at [SPEC-SUI-227]")
    sampo_build = {"available": True, "commit": "a" * 40, "commit_short": "aaaaaaaaaaaa"}
    srv = fake_vaino()  # build_json=None -> /build 404s
    try:
        result = vaino_control.ensure_vaino(port=srv.server_port, sampo_build=sampo_build)
        check(result == {"ok": True, "port": srv.server_port, "started": False},
              f"an unreadable /build must not block a capable handoff, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("Sampo itself has no git checkout: unknowable, so skipped rather than "
          "guessed at [SPEC-SUI-227]")
    srv = fake_vaino(build_json={"git": "b" * 12, "branch": "main", "commit_date": "2026-08-30"})
    try:
        result = vaino_control.ensure_vaino(port=srv.server_port, sampo_build={"available": False})
        check(result == {"ok": True, "port": srv.server_port, "started": False},
              f"an unknown Sampo commit must not block a capable handoff, got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("a bare listening socket with no HTTP behind it is not mistaken for a working Vaino")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as bare:
        bare.bind(("127.0.0.1", 0))
        bare.listen(1)
        bound = bare.getsockname()[1]
        check(vaino_control._vaino_reachable(bound), "the socket question alone must still say reachable")
        check(not vaino_control._vaino_has_sampo_support(bound, timeout=0.5),
              "a connection that never speaks HTTP must not read as a sampo-support build")

    print()
    print("no library open: refused, not guessed past")
    result = vaino_control.ensure_vaino(port=free_port(), db_path=None)
    check(result["ok"] is False, f"expected a refusal, got {result}")
    check("no library" in result.get("error", "").lower(),
          f"the reason must name what is missing, got {result}")

    print()
    print("no local binary: named as the missing capability, not a crash")
    old_which = vaino_control._vaino_binary
    vaino_control._vaino_binary = lambda: None
    try:
        result = vaino_control.ensure_vaino(port=free_port(),
                                             db_path=os.path.join(HERE, "..", "test_vaino.db"))
        check(result["ok"] is False, f"expected a refusal, got {result}")
        check("binary" in result.get("error", "").lower(),
              f"the reason must name what is missing, got {result}")
    finally:
        vaino_control._vaino_binary = old_which

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("console handoff: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
