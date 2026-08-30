# IMPL008: Targeted Remote Reads at Edit-Open Time

**Implementation Guide — the build order for [SPEC022 §2](spec/SPEC022-flag-and-edit-sync.md#2-a-console-gui-for-1-its-measured-cost-and-a-design-to-keep-it-fast)'s `[SPEC-DF-116..118]`, settled design, nothing built**

> **Related:** [SPEC022](spec/SPEC022-flag-and-edit-sync.md) — read this first, in full, it is short · `tools/apply_changes.py` (the anchors and `classify()` this reuses) · `tools/console.py`, `tools/console_web/profile.html` (where this lands) · commit `abb41c0` (the pull/push GUI this builds past)

---

## 1. What already exists — do not re-derive, read SPEC022 instead

A GUI for pulling vainopi's flags and pushing local edits back is already built and committed (`tools/jobs.py`'s `remote-pull`/`remote-push`, the "Sync with a remote" section in `tools/console_web/flags.html`). It works, but both directions cost a full `scp` of vainopi's ~1.16 GB database — **measured live at over an hour**, because vainopi's WiFi sustains roughly 270 KB/s. The write itself was never the expensive part (already a small SQL patch); the *read* was, because `apply_changes.py`'s three-way merge needs the remote's current value to classify a change as fast-forward, no-op, or conflict.

**Editing itself lives in Vaino, not Sampo.** `console.py`'s `/profile/:id` route is, and stays, read-only — the actual id-review/boundary editing UI is Vaino's own web UI (port 5720), reached via a handoff link from Sampo's profile page.

## 2. What this builds: one targeted read, at the right moment

**The read itself, per `[SPEC-DF-116]`:** vainopi already carries the `sqlite3` CLI (confirmed live: 3.40.1, `-json` output works — `ssh pi@vainopi "sqlite3 -json /srv/library/vaino.db '...'"` returns a JSON array directly). Use it for a **per-item** query instead of a database copy: when Sampo's profile page for one passage/recording is opened, fetch just that item's current remote value via the identical anchors `apply_changes.py` already resolves locally (`audio_md5`+kind+start/end for a boundary, `recording_mbid` for an artist/id review) — not a copy, a handful of bytes.

**Offering it, per `[SPEC-DF-117]`:** if the remote's value differs from what Sampo's own library shows, offer it as the new starting point *before* the "Review in Vaino" handoff — not after, not only at push time. Accepting it is a small, targeted **local** write (the one deliberate exception to "the console never writes the library"), and it must follow the existing discipline: not `console.py`'s HTTP handler touching SQLite directly, but a dedicated subprocess tool doing exactly that one row's update — the same shape every other write in this console already takes (`jobs.py`'s `_spawn`).

**Degrading gracefully, per `[SPEC-DF-118]`:** if vainopi doesn't answer, show *"could not reach vainopi — editing against the local baseline only"* and proceed. Never block. Household WiFi to this hardware has already proven unreliable all through the Sonos investigation on this same branch history — a check that can't run must not stop someone from working.

## 3. Build order

1. **`tools/remote_peek.py`** — the one new read primitive everything else depends on. Given a remote (`user@host:/path`) and one anchor (recording mbid, or `audio_md5`+kind+start/end), builds the right `SELECT` and runs it via `ssh <host> sqlite3 -json <path> "..."`, returning the parsed row or `None`. Mirror `apply_changes.py`'s own `current_recording()`/`current_boundary()`/`current_artist()` for what "current value" means per kind — reuse those shapes, don't reinvent them. Handle unreachable/timeout explicitly (a caught exception or non-zero exit, not a hang) — `[SPEC-DF-118]` depends on this failing cleanly and fast.
2. **A small write tool** (name it plainly, e.g. `tools/accept_remote_basis.py`) — given a local db, an anchor, and the remote's value just fetched, writes it as the new local value for exactly that row. Small and boring on purpose; it exists only so `console.py` itself never opens the library for writing.
3. **`console.py` + `profile.html` wiring** — on opening a profile, call `remote_peek.py` (as a subprocess, matching every other write/read-adjacent action in this console) against the configured remote (already stored — `STATE["jobs"].get_remote()`, from the work already committed). Three outcomes to render: unreachable (warning, proceed), no divergence (nothing to show), diverged (offer the choice, and call the accept-tool if taken).

## 4. What "done" looks like — a measurable claim, not a demo

- A profile page opened while vainopi is reachable and in agreement shows nothing extra.
- A profile page opened for an item deliberately diverged on vainopi (set a flag or edit directly there, or against a scratch copy first) shows the difference and correctly writes the accepted value locally when taken.
- A profile page opened with vainopi unreachable (pull the network cable, or point the remote config at a bad host) shows the warning and does not block reaching Vaino's own editor.
- After accepting a remote value and then making a local edit, a subsequent `remote-push` (already built) classifies it as fast-forward, not conflict — this is the actual point of the feature, worth checking end to end once, not assumed from the pieces working separately.

---

**Traceability:** implements `[SPEC-DF-116..118]` · depends on `abb41c0` (already committed) · nothing in this document is built yet
