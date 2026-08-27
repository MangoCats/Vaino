#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for the Sampo→Vaino handoff in `console.py` [SPEC-SUI-170].

Exercises the three-step contract on its own -- liveness is a socket
question, not a route question; a missing library or a missing binary is
reported, not guessed past; an already-reachable port is used, never
duplicated -- without needing a real Vaino binary on the machine running the
test.

    python tools/test_console_handoff.py
"""

import os
import socket
import sys

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
    print("already running: used, not duplicated")
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as srv:
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        bound = srv.getsockname()[1]
        result = console.ensure_vaino(port=bound)
        check(result == {"ok": True, "port": bound, "started": False},
              f"an already-reachable port must be reported as such, got {result}")

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
