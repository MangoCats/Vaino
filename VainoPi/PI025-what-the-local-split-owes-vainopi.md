# PI025: What the Local Split Owes vainopi

**Appliance Record — a running register, opened 2026-09-11**

Splitting the *local* database is turning up defects that vainopi already
has, because vainopi split first and nothing swept the rest of the system
afterwards. Each one is found on the desktop, fixed on the desktop, and
then owed to the appliance — which is running today and is not going to be
disturbed for every one of them individually.

This is that debt, itemised. It exists so the fixes land on `vainopi` as
one deliberate update rather than as four separate recollections.

> **Related:** [PI023](PI023-the-database-split-on-disk.md) for the split on
> disk · [IMPL011](IMPL011-database-split-built.md) for the migration as it
> ran · [IMPL009 §7.7](IMPL009-database-split-plan.md) for the prerequisite
> the local split had to build first

---

## 1. The pattern, stated once

**`[PI-OWE-010]`** Every item below is the same omission wearing different
clothes: **the split was done thoroughly for the player's main data path
and not swept across everything else that touches a database.** The player
got `QualifyingConn` and `__LIB__`; four folder-writing generators did not.
`split_database.py` copied schema and rows; it did not copy the journal
mode. `tools/` got nothing at all. `[IMPL002 §7.4]`'s second peer path was
stored and never read.

Recorded as a pattern and not four coincidences, because the next thing
that touches a database on a split installation will be the fifth unless
somebody goes looking.

## 2. Owed to vainopi

**`[PI-OWE-020]` The four folder-writing generators are broken there now,
and `covers`/`cue_sheets` are both switched on.** `run_generation`
(`player/src/bin/vaino.rs`) opened the path the player was *started* with —
the listener half — while `cue`/`covers`/`lyrics_sidecar`/`lyrics_cache`
read catalogue tables and nothing else. On vainopi that is
`no such table: passages` on the first statement, and has been since the
appliance split. Verified directly against `/var/vaino/listener.db`
2026-09-11, with `covers=1` and `cue_sheets=1` in `player_settings`.

**Fixed locally 2026-09-11** — the generators now open the catalogue half,
which is the same file on an unsplit installation and changes nothing
there. **Needs a build and a deploy to reach vainopi**, so it is the one
item here that cannot be applied with `sqlite3` alone.

**`[PI-OWE-030]` vainopi lost WAL in the split, and that is why its
database intermittently reports "locked".** The pre-split
`/srv/library/vaino.db` is `wal`; both live halves are `delete`. A fresh
SQLite file is `delete` and `split_database.py` never set the mode, so the
migration silently changed it. Measured 2026-09-11: a plain
`SELECT COUNT(*)` against `listener.db` over `ssh` failed once, seconds
after the service restarted, and succeeded four times immediately after —
which is exactly what `delete` mode does when a reader meets the writer,
and what WAL exists to avoid.

Fixed in `split_database.py` for future splits. For vainopi it is
`PRAGMA journal_mode = WAL` on each half — persistent, no data change, no
rebuild. **Check first whether anything there writes both halves in one
transaction**: SQLite documents a cross-database transaction as atomic only
when the journal mode is *not* WAL. Nothing does today (the player attaches
the catalogue read-only), which is what makes this safe there and would not
make it safe everywhere.

**`[PI-OWE-040]` Two empty shadow tables sit in vainopi's listener half.**
`file_tags` (0 rows) and `cover_art` (0 rows) are in `/var/vaino/listener.db`
while the real ones — 5,709 and 1,079 rows — are in `/srv/library/library.db`.
The player opens the listener half as `main`, so an unqualified reference to
either resolves to the empty copy.

Harmless **today**: `library.rs` qualifies every catalogue table with
`__LIB__.`, so nothing reads them. It is a loaded gun rather than a wound,
and the two generators in `[PI-OWE-020]` are what a pulled trigger looks
like. Drop both from the listener half; nothing there should own them.

`schema_meta` is in both halves **on purpose** (`split_database.py`'s own
`BOTH` list) and must not be dropped — the distinction is real and
`tools/vaino_db.py` encodes it as `SHARED_TABLES`.

**`[PI-OWE-050]` The console's sync-preferences job could not reach a split
peer.** `[IMPL002 §7.4]` designed the two-path model and it was half built:
`sync_peers.remote_listener`, `sync_remote_listener` and
`JobRunner.get_remote_listener()` all existed, with tests, and nothing read
them. Fixed locally 2026-09-11; the desktop's own `sync_remote` was also
still naming vainopi's pre-split `/srv/library/vaino.db`, a file two days
stale that nothing reads. Nothing is owed *to* vainopi here — the fix and
the misconfiguration were both on this side — recorded so the appliance is
not suspected of it later.

**`[PI-OWE-055]` Pulling flags from vainopi reported "nothing flagged" for
a peer with 19.** `remote_flags.py` is the third script `[IMPL002 §7.4]`
named, and the last to be fixed. Against a split peer it failed in both
directions: pointed at the listener half, `no such table: passages`;
pointed at the catalogue half, `no such table: listener_flags` — which its
own guard, written for "no Vaino carrying `[REQ-VIS-265]` has opened this
library yet", turned into a cheerful empty result.

Fixed by attaching the peer's listener half in front of the query, the one
thing `run_remote_sql` can do with a second path since `sqlite3` takes more
than one statement. Measured against `pi@vainopi` 2026-09-11: 0 flags
before, **19 after**, matching what the appliance actually holds. The guard
now fires only when no listener path was given, so "absent from this file"
can no longer masquerade as "absent from this installation".

Nothing is owed *to* vainopi — the defect was on this side — but it is the
clearest instance yet of `[PI-OWE-010]`'s pattern, and the reason that
pattern is stated as a pattern.

## 3. Not owed, checked anyway

**`[PI-OWE-060]` The cascade replacement is not needed on vainopi.**
`[IMPL009 §7.7]`'s `ON DELETE SET NULL` cascade is just as dead there as
here, but nothing on vainopi deletes passages — segmentation is desktop-only
per `[SPEC035]`. Checked 2026-09-11 rather than assumed: **zero orphans**
across all three of `listener_play_history`, `selection_decisions` and
`player_state`. `tools/passage_orphans.py` is worth keeping as a
diagnostic there, not as scheduled work.

**`[PI-OWE-065]` The two-phase commit in the `apply_*` scripts is desktop-only
for the same reason.** Those three write the catalogue and then stamp a
listener-side review table, which on a split pair is two files and — under
WAL — not one atomic act. They now commit catalogue-first so the only
survivable half-failure is one a re-run repairs. vainopi runs none of them,
so it inherits nothing here; recorded because the *reasoning* applies to
anything that ever writes both halves there, and nothing does yet.

**`[PI-OWE-070]` No catalogue table is missing from vainopi's catalogue
half, and no listener table has strayed into it.** Checked the same day,
both directions. The only duplicates are the two in `[PI-OWE-040]` and the
deliberate `schema_meta`.

---

## 4. Open

**`[PI-OWE-080]`** None of the above has been applied to vainopi. Three of
the four are `sqlite3` one-liners; `[PI-OWE-020]` needs a build and a
deploy. Doing them together, once, is the point of this document.

**`[PI-OWE-090]`** `bose` is not split and is therefore untouched by all of
it — but `[PI-OWE-010]`'s pattern is what it will meet on the day it is,
and this register is the list to re-run against it then.

---

**Traceability:** `[PI-OWE-010..090]` · follows [PI023](PI023-the-database-split-on-disk.md)
and [IMPL011](IMPL011-database-split-built.md) · carries
[IMPL009 §7.7](IMPL009-database-split-plan.md)'s prerequisite and
`[IMPL002 §7.4]`'s peer model forward to the appliance · opened while
splitting the local database, which is what found every item in it
