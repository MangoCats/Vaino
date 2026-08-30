#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the Sampo→Vaino handoff in `console.py` [SPEC-SUI-170],
[SPEC-SUI-213].

Exercises the contract on its own -- liveness is a socket question, not a
route question; a missing library or a missing binary is reported, not
guessed past; an already-reachable port is used, never duplicated; and, per
`[SPEC-SUI-213]`, a *reachable* port whose Vaino was built without
`--features sampo-support` is named as such rather than left to 404 on the
next click -- without needing a real Vaino binary on the machine running the
test. A minimal fake HTTP server stands in for the two builds that matter:
one that serves `/review.js`, one that 404s it, both otherwise identical.

    python tools/test_console_handoff.py
"""

import http.server
import os
import socket
import sys
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import console  # noqa: E402

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
    one that matters to `_vaino_has_sampo_support`.
    """
    review_status = 200

    def log_message(self, fmt, *args):
        pass  # quiet -- a passing test has nothing to say

    def do_GET(self):
        status = self.review_status if self.path == "/review.js" else 200
        self.send_response(status)
        self.send_header("Content-Length", "0")
        self.end_headers()


def fake_vaino(review_status: int) -> http.server.HTTPServer:
    handler = type("Handler", (_FakeVaino,), {"review_status": review_status})
    srv = http.server.HTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv


def main() -> int:
    print("liveness is a socket question, not a route question")
    port = free_port()
    check(not console._vaino_reachable(port, timeout=0.2),
          "a port nothing is listening on must read as unreachable")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        bound = srv.getsockname()[1]
        check(console._vaino_reachable(bound, timeout=0.5),
              "a port something is listening on must read as reachable")

    print()
    print("already running, and it has the routes this handoff needs: used, not duplicated")
    srv = fake_vaino(review_status=200)
    try:
        port = srv.server_port
        result = console.ensure_vaino(port=port)
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
        result = console.ensure_vaino(port=port)
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
        result = console._vaino_ready(srv.server_port, started=True)
        check(result["ok"] is False and result["started"] is True, f"got {result}")
        check("Sampo just started" in result.get("error", ""), f"got {result}")
    finally:
        srv.shutdown()
        srv.server_close()

    print()
    print("a bare listening socket with no HTTP behind it is not mistaken for a working Vaino")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as bare:
        bare.bind(("127.0.0.1", 0))
        bare.listen(1)
        bound = bare.getsockname()[1]
        check(console._vaino_reachable(bound), "the socket question alone must still say reachable")
        check(not console._vaino_has_sampo_support(bound, timeout=0.5),
              "a connection that never speaks HTTP must not read as a sampo-support build")

    print()
    print("no library open: refused, not guessed past")
    old_path = console.STATE["path"]
    console.STATE["path"] = None
    try:
        result = console.ensure_vaino(port=free_port())
        check(result["ok"] is False, f"expected a refusal, got {result}")
        check("no library" in result.get("error", "").lower(),
              f"the reason must name what is missing, got {result}")
    finally:
        console.STATE["path"] = old_path

    print()
    print("no local binary: named as the missing capability, not a crash")
    old_path = console.STATE["path"]
    old_which = console._vaino_binary
    console.STATE["path"] = os.path.join(HERE, "..", "test_vaino.db")
    console._vaino_binary = lambda: None
    try:
        result = console.ensure_vaino(port=free_port())
        check(result["ok"] is False, f"expected a refusal, got {result}")
        check("binary" in result.get("error", "").lower(),
              f"the reason must name what is missing, got {result}")
    finally:
        console.STATE["path"] = old_path
        console._vaino_binary = old_which

    print()
    if FAILED:
        print(f"{len(FAILED)} check(s) failed")
        return 1
    print("console handoff: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
