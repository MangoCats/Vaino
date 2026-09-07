# IMPL002: The `[PI-DB-010]` Database Split, in Detail

**Implementation Specification — Tier 2 · DESIGNED, NOT YET BUILT**

Everything needed to actually build the split [PI001 §5](PI001-image-and-partitions.md#5-the-database-split)
has described since early in the project and neither appliance has built:
one `vaino.db` becoming two files, `library.db` (B) and `listener.db` (C).
This document is what PI001 left as "design, not code" made concrete enough
to implement — the exact tables, the exact call sites in the player today,
the migration procedure, and what a review against synchronization, RAM,
backup, and MPD turned up.

> **Related:** [PI001 §5](PI001-image-and-partitions.md#5-the-database-split) ·
> [SPEC008 schema](../docs/spec/SPEC008-database-schema.md) ·
> [SPEC035 mesh sync](../docs/spec/SPEC035-mesh-library-sync.md) ·
> [BOSE002 `[IMPL-BOS-078]`](../BosePi/BOSE002-image-build.md#L110), the
> reason `bose` did *not* build this and put the whole file on C instead

---

## 1. Why now, and why vainopi first

Asked directly whether splitting vainopi's database would complicate the
mesh-sync tooling built for `[SPEC035]`: it doesn't. `tools/mesh_diff.py`
and `tools/resolve_mesh_conflict.py` already address each side of a diff by
an independent `(path, table)` pair drawn from a hardcoded `TABLES` allowlist
(`files`, `recordings`, `passages` — never a `listener_*` name), so a split
target just means pointing the tool at `library.db` instead of `vaino.db`.

**`[IMPL-DBSPLIT-005]` No change to either sync tool is needed for this.**
That finding is *why* this is safe to build now rather than a reason to
build it — see §5 for the review in full.

vainopi, not bose, is the right first target: bose deliberately substituted
the whole file on C (`[IMPL-BOS-078]`) specifically because building it for
real showed B genuinely goes read-only there, and moving the split's own
work onto a second appliance mid-way through would confuse which machine is
testing what. vainopi's B never becomes read-only in the current design
(`[PI-A-020]`'s successor discussion), so it does not need the same
workaround — it can build the split PI-DB-010 actually describes.

---

## 2. The actual code today: three connections, one file

Reading `[PI-DB-030]`'s claim of "four queries cross the boundary" against
the player source directly (not against the doc) found the real shape is
bigger, but still bounded and enumerable.

**`[IMPL-DBSPLIT-010]` As of this writing, `vaino` opens the *same*
`vaino.db` path three separate times**, for three different reasons:

| Connection | Opened in | Mode | Owns |
| :--- | :--- | :--- | :--- |
| `Library` | `db/library.rs::open()` | `SQLITE_OPEN_READ_ONLY` | catalog browsing, passage lookup, **and** several listener-joined reads |
| `PlayerStore` | `db/player_store.rs::open()` | read-write | `player_state`, listener history/rejection/flags/preferences tables — **and creates two catalog-side tables and one catalog-side column set as a side effect of being "the only writable handle"** |
| a bare `Connection` | `bin/mpd_direct.rs` (the real binary) | default (rw) | feeds `Director::load()`, which reads catalog (`flavor`, `programs`, relations) and listener (`listener_likes`, `listener_preferences`, `listener_settings`) **together, unqualified, in the same queries** |

**`[IMPL-DBSPLIT-015]` Every real (non-test) cross-boundary reference,
enumerated rather than estimated:**

- `db/library.rs`: the `DESCRIBE` constant (play count + last-played, 2
  references), `PLAYS_EXPR` (1), `count_radio`/`play_history`/
  `play_history_count` (5, via `listener_flags`/`listener_play_history`/
  `listener_rejections`) — 8 sites, all reads.
- `director/library.rs::load_taste()`: 2 sites (`listener_likes`,
  `listener_preferences`), each joined in-process against `flavor`
  (catalog) via `FlavorIndex`, not in SQL — no JOIN to rewrite, just the two
  raw `SELECT`s to qualify.
- `backup.rs::restore()`: the class-D bundle-restore path. Already
  cross-database (`ATTACH ... AS snap`) for a *different* reason — comparing
  an imported snapshot against the live file — and needs re-examination
  once the live side is itself two files (§4).
- `PlayerStore::open()`: **not a query to qualify, an ownership question.**
  It creates `file_tags` (`TAG_TABLE`) and `cover_art` (`ART_TABLE`) — both
  listed as B-side tables in PI001 §5's own table — and `ALTER TABLE
  release_recordings ADD COLUMN chosen/position/disc`, altering a catalog
  table. The doc comment names the reason honestly: *"Created here because
  this is the player's only writable handle."* That assumption is exactly
  what a split removes. See §4.3.

Nothing found here contradicts `[PI-DB-010]`'s design — the boundary the
schema already draws (the `listener_` prefix) is still the right line. What
this section corrects is the doc's undersell of how many places currently
assume there is no boundary at all.

---

## 3. File layout and the ATTACH mechanics

**`[IMPL-DBSPLIT-020]` Two files, exactly the tables PI001 §5 already
named:**

| File | Partition | Tables |
| :--- | :--- | :--- |
| `library.db` | B, `/srv/library/library.db` | `files`, `passages`, `passage_recordings`, `recordings`, `artists`, `releases`, `release_recordings`, `flavor`, `cover_art`, `file_tags`, `id_checks`, `programs`, the caches (`lowlevel_cache`, `musicbrainz_cache`, `identification_cache`) |
| `listener.db` | C, `/var/vaino/listener.db` | `player_state`, `player_settings`, `listener_play_history`, `listener_rejections`, `listener_flags`, `listener_preferences`, `listener_likes`, `listener_settings`, `id_reviews`, `selection_decisions` |

**`[IMPL-DBSPLIT-025]` One connection, one ATTACH, matching `[PI-DB-020]`
exactly: the player opens `listener.db` and attaches `library.db` read-only,
never the reverse.**

```sql
ATTACH DATABASE 'file:/srv/library/library.db?mode=ro' AS lib;
PRAGMA main.synchronous = FULL;      -- listener state, per [PI-C-050]
PRAGMA main.journal_mode = WAL;
```

Concretely, this means collapsing today's three separate connections
(`Library`, `PlayerStore`, the bare one in `mpd_direct.rs`) into **one
attach-capable connection type**, or at minimum having each of the three
open `listener.db` and issue the same `ATTACH` rather than three
independent opens of the same combined file. The natural home for this is a
new small wrapper — `db::AttachedDb::open(listener_path, library_path)` —
that every one of the three current call sites constructs instead of
calling `Connection::open` directly. `Library` and `PlayerStore` keep their
existing method surfaces; only what they hold internally changes.

**`[IMPL-DBSPLIT-030]` CLI surface: a new explicit flag, not a directory
convention.** `vaino`'s existing flags are all explicit (`--device`,
`--mpd-root`) rather than inferred from a shared parent directory, and
`[PI-SET-020]`'s whole argument against implicit conventions applies here
too. Add `--library <path>` alongside the existing positional/`--db`
listener-database argument; `vaino-bose.service`-style units on vainopi
gain one more `ExecStart` argument, nothing structural.

**`[IMPL-DBSPLIT-035]` Query qualification is mechanical, not invented per
site.** Every reference to a table in the `library.db` column of §3's table
gets a `lib.` prefix; every `listener_*`/`player_*`/`id_reviews`/
`selection_decisions` reference is already correct unqualified, since it
resolves to `main` by default. The 8+2 sites enumerated in `[IMPL-DBSPLIT-015]`
are exactly the ones that mix both in one statement and need the prefix on
one side only.

---

## 4. What the review against applicable concerns found

### 4.1 Synchronization — no complication, confirmed against the actual tool

Already answered directly and re-stated here for the record
(`[IMPL-DBSPLIT-005]`): `mesh_diff.py`/`resolve_mesh_conflict.py` never
touch a `listener_*` table, and address each side by an independent path.
Post-split, the peer registry's stored path for vainopi changes from
`/srv/library/vaino.db` to `/srv/library/library.db` — a configuration
edit, not a code change. A **mixed mesh** (bose still one file, vainopi
split) works with zero tool changes, since the two sides of a diff have
never been required to share a layout.

### 4.2 RAM — negligible, and not the risk in this design

Two small `sqlite3` connections instead of one cost a few hundred KB of
page-cache overhead each, immaterial against `[REQ-HW-100]`'s 150 MB
budget and dwarfed by the PipeWire/WirePlumber/BlueZ stack already running
on vainopi. `library.db` is opened `mode=ro` via the `ATTACH` URI, so no
separate writable-open memory cost exists on that side at all. **This split
does not interact with the overlay-RAM risk found on `bose`
(`[IMPL-BOS-165]`)** — vainopi has no overlay yet, and if one is added
later, `recurse=0` scoping the overlay to root alone (§6 of PI001,
already planned regardless of this split) is what controls that risk, not
the database layout.

### 4.3 The real finding: `PlayerStore` cannot stay "the only writable handle"

`file_tags`, `cover_art`, and `release_recordings`'s Sampo-selection
columns are populated by tools that are not the player at all —
`tagscan`, `tools/fetch_cover_art.py`, and Sampo's own release-selection
step. `PlayerStore::open()` creates/migrates them today only because, with
one file, it happens to be the sole writable connection available at
runtime. Once B and C are separate files, `PlayerStore`'s writable open is
scoped to `listener.db` and has no writable path to `library.db` at all —
by design, per `[PI-B-010]`, B is read-only except during an attended
import.

**Resolution, corrected in `[§7.6]` after a cross-environment check found
"entirely" was too strong: conditional on whether this installation has
actually split.** When `db_path == library_path` (bose, local, every
unsplit installation) `PlayerStore`'s connection *is* the library's own
writable connection, exactly as today, and removing its bootstrap
unconditionally would have broken a fresh single-file library exactly the
way the original doc comment warned against. Only when the two paths
genuinely differ does `PlayerStore` skip these statements (it has no
writable path to `lib` to run them against) — moved instead to the
migration tool (§8) for whatever the source already had at split time, and
to `tagscan`/`fetch_cover_art.py` making their own bootstrap
self-sufficient (`fetch_cover_art.py` already is; `tagscan` needs the same
fix — see `[§7.6]`).

### 4.4 Backup — smaller and more frequent becomes possible, not required

`[PI-C-030]`'s off-device backup requirement gets cheaper: `listener.db`
alone is the ~2.4 MB figure PI001 §5 already cites, not the ~1 GB combined
file. `library.db` still needs backing up, but it changes only on a
deliberate import — the same rare, attended event `[PI-B-030]` already
describes — so it can be backed up on that event rather than on the
interval `listener.db` deserves. No requirement changes; the split just
lets the two files follow schedules that already match how often each one
actually changes.

### 4.5 MPD — unaffected, confirmed by design, not merely assumed

MPD's own database (its playback index, distinct from `vaino.db`) already
lives on B per BOSE002's own note ("MPD's own database — its index —
belongs here too and not on A") and is addressed by `mpd.conf`'s `db_file`
directly, never through `vaino`'s connection. Splitting `vaino.db` touches
nothing MPD reads.

---

## 5. Migration procedure

**`[IMPL-DBSPLIT-040]` The tool: `tools/split_database.py`, following this
project's existing rehearse-by-default shape** (`[SPEC-DF-109]`, the same
convention `resolve_mesh_conflict.py` and `export_bundle.py` already use).

```
python tools/split_database.py vaino.db --library-out library.db --listener-out listener.db
python tools/split_database.py vaino.db --library-out library.db --listener-out listener.db --commit
```

1. Open the source `vaino.db` **read-only** — this tool never writes to
   the file it's splitting.
2. Create `library.db` fresh; copy every table in §3's B column via
   `CREATE TABLE ... AS SELECT * FROM source.<table>` over an `ATTACH`,
   preserving indexes and the schema Sampo/tagscan/fetch_cover_art expect —
   including `file_tags`/`cover_art`/`release_recordings`'s tuning columns,
   per `[IMPL-DBSPLIT]` 4.3's resolution, created here instead of at
   player runtime from now on.
3. Create `listener.db` fresh the same way for every table in the C column.
4. **Verify before anything is swapped in**: row counts on both new files
   match the source table-for-table; spot-check a handful of `listener_play_history`
   rows and `passages` rows by primary key against the source; confirm
   `library.db`'s `PRAGMA integrity_check` and `listener.db`'s both pass.
5. Report only, unless `--commit`. `--commit` is the only mode that writes
   the two output files to their final names — dry-run writes to a
   temp directory and deletes it, so a rehearsal never leaves half-built
   files where the real ones would go.

**`[IMPL-DBSPLIT-045]` This tool never deletes the source file.** The
original `vaino.db` is left exactly where it was, renamed with a
timestamp suffix, not removed — the same "keep the old thing until the new
thing is proven" discipline `request-unlock.sh`'s recovery and
`seed-library.sh`'s `--force-swap` guard both already use. Reverting a bad
split is: stop `vaino`, point its unit file's arguments back at the single
old file (dropping `--library`), restart. No data is at risk from the
split tool itself failing partway, because it only ever writes to files
that do not yet exist where the running system reads from.

---

## 6. Player refactor: scope, not yet built

**`[IMPL-DBSPLIT-050]` Enumerated, bounded, not yet implemented:**

- New `db::AttachedDb` (or equivalent) wrapping the `ATTACH` open, replacing
  the three independent `Connection::open`/`Connection::open_with_flags`
  call sites in `library.rs`, `player_store.rs`, and `bin/mpd_direct.rs`.
- 8 call sites in `db/library.rs` (§2) and 2 in `director/library.rs`
  get a `lib.` prefix on their catalog-table references.
- `PlayerStore::open()` loses the `TAG_TABLE`/`ART_TABLE` creation and the
  `release_recordings` `ALTER`s (§4.3); those move into
  `tools/split_database.py` (for a fresh split) and into whatever
  `tagscan`/`fetch_cover_art.py` already do when opening B writable (for a
  library.db that already exists and gains a tool it hasn't seen before).
- `backup.rs::restore()` needs its own pass: it currently reasons about
  "the live file" as one thing to compare a snapshot against, and after the
  split that reasoning must say which of the two live files a given
  restored table belongs to. Not yet worked through in detail — flagged
  here rather than glossed over.
- Every test fixture across these files that builds one in-memory or
  temp-file connection with both table families present needs either a
  second attached temp file or an equivalent in-memory `ATTACH` — this is
  the largest *count* of individual edits (dozens of test setup blocks),
  though each one is mechanical once the pattern is established for the
  first.

**This is real, multi-file, cross-cutting work — sized honestly as at least
one focused implementation pass with its own test cycle, not a
same-turn addition to a design review.** Building it carefully, with the
project's own TDD discipline and full regression between changes, is the
right next step; committing it hastily onto a live appliance holding
irreplaceable listener history (`[PI-C-020]`) is exactly the failure mode
this whole design exists to avoid.

---

## 7. Implementation plan, reviewed for gaps before starting

Asked directly to plan the implementation carefully and review that plan
for gaps, oversights, conflicts, and ambiguities before proceeding. Doing
that review against the actual schema and the actual tools directory (not
just against §1–6 above) found four more real problems — one of them a
genuine conflict this document's own §3 would otherwise have walked into.
Each is stated as found, then fixed.

### 7.1 Gap: `CREATE TABLE ... AS SELECT` silently drops schema, not just data

§5's migration sketch said "copy every table... via `CREATE TABLE ... AS
SELECT * FROM source.<table>`." Checked against the actual schema rather
than left as a plausible-sounding sentence: **that statement does not
preserve indexes, `UNIQUE`/`CHECK`/`NOT NULL` constraints, `DEFAULT`
values, or foreign keys** — SQLite's own documented behavior, not a bug —
it copies column names and data only.

This is not a theoretical loss. `bundle.rs` creates
`CREATE UNIQUE INDEX passages_span ON passages(file_id, kind, start_ms, end_ms)`
— the constraint that stops a re-import from silently duplicating a
passage. `player_store.rs` creates four more (`idx_file_tags_album`,
`idx_file_tags_artist`, `idx_release_recordings_chosen`, plus the two on
`listener_play_history` and one on `listener_rejections`). A split built
exactly as §5 first described would produce a `library.db` that accepts
duplicate passages the source database never could.

**Fix:** the migration tool reads each table's real DDL from
`sqlite_master` (`CREATE TABLE`/`CREATE INDEX`/`CREATE TRIGGER` text,
verbatim) and executes that on the new file *before* copying data with
`INSERT INTO ... SELECT`, table by table. §5 is corrected below to say this
instead of `CREATE TABLE ... AS SELECT`.

### 7.2 Checked, not assumed: no foreign key crosses the boundary

SQLite does not enforce a `FOREIGN KEY` across two attached databases —
only within one file. Before assuming this doesn't matter, checked every
`REFERENCES` in the schema: the only two are `recording_artists.mbid
REFERENCES recordings(mbid)` and `recording_artists.artist_mbid REFERENCES
artists(mbid)` — both tables land on `library.db`, so both stay
enforceable exactly as they are today. **No table on either side
references a table that ends up on the other side.** Stated here as a
checked fact so a future schema change that *does* add a cross-boundary
reference has something to contradict, rather than the split silently
having assumed this forever.

### 7.3 Conflict: the query rewrite in §3 would have broken `bose`

§3 said every catalog reference "gets a `lib.` prefix." Read literally,
that rewrite ships in the one player binary both appliances run — `bose`
included, which is staying single-file (`[IMPL-BOS-078]`) for the
foreseeable future. A hardcoded `lib.` prefix has no `lib` schema to
resolve against unless something is always attached under that name, and
attaching a database's *own* file to itself under a second alias is not a
pattern this design should lean on untested — SQLite documents `ATTACH`
for a second file, and a same-file self-attach's locking behavior under a
real read/write connection was not going to be verified by assuming it
works.

**Fix:** qualify with a runtime alias, not a hardcoded string. One shared
helper —

```rust
/// Attaches `library_path` as `lib` when it differs from the connection's
/// own file, and returns the schema prefix every catalog query should use.
/// The two paths are equal for every installation that hasn't split yet
/// (bose, today's vainopi, every existing test fixture) -- in that case
/// nothing is attached and this returns "main", so a query written
/// `{LIB}.recordings` reads the same table through the same connection it
/// always has. Only an installation with a genuinely separate library.db
/// ever attaches anything or ever sees "lib" come back.
pub fn attach_library(conn: &Connection, db_path: &Path, library_path: &Path)
    -> Result<&'static str, DbError>
```

— used by all three of today's independent connection-opening sites
(`Library::open`, `PlayerStore::open`, the bare `Connection::open` in
`bin/mpd_direct.rs`), so the attach/alias decision is made once and cannot
drift between them. Every one of the ~10 call sites in `[IMPL-DBSPLIT-015]`
is rewritten to interpolate this alias (`format!("... FROM {lib}.recordings ...")`)
rather than a literal `lib.`. `bose` keeps running the same binary,
unmodified, forever — its two paths are just equal.

This also simplifies §5's rollback story: reverting vainopi's split needs
no binary swap, only pointing `--library` back at the same path as `--db`
(or dropping the flag, with the default being "same as `--db`") — the
binary already handles that case as its normal, most-tested path.

### 7.4 Conflict found in `tools/`: one remote path cannot serve two files

The synchronization review in §4.1 checked `mesh_diff.py` and
`resolve_mesh_conflict.py` and found them clean — they only ever touch
catalog tables. Checking the *rest* of `tools/` before calling
synchronization fully reviewed found three more scripts that assume one
remote path gives access to **both** halves at once:

- `sync_preferences.py` syncs `listener_preferences` (listener-side) but
  its own docstring requires checking "a real artist/recording on *both*
  sides" — a catalog-side existence check — through the same connection.
- `remote_flags.py` and `export_flags.py` fetch flagged recordings/passages
  by joining `listener_flags` (listener-side) against `recordings`/
  `passages` (catalog-side).

All three reach a peer through `tools/jobs.py`'s `sync_peers.remote` /
`remote_config`'s single `sync_remote` value — one string, `user@host:/path`.
Once vainopi is split, that one path is either `library.db` (and these
three tools lose the listener tables they need) or `listener.db` (and they
lose the catalog tables) — it cannot be both. This is a real conflict
`[IMPL-DBSPLIT-005]`'s "no sync-tool changes needed" did not cover, because
`mesh_diff`/`resolve_mesh_conflict` were the only two tools actually
checked at the time.

**Fix:** `sync_peers` gains a second, nullable column,
`remote_listener TEXT` (`ALTER TABLE sync_peers ADD COLUMN remote_listener
TEXT`, additive and safe against every existing row), read as "same file as
`remote`" when `NULL` — true for every peer that hasn't split, `bose` and
today's vainopi included, so nothing already configured needs re-entering.
`remote_config` gets the equivalent second key,
`sync_remote_listener`. `sync_preferences.py`/`remote_flags.py`/
`export_flags.py` each take the listener path from the new field and the
catalog path from the existing one; `mesh_diff.py`/`resolve_mesh_conflict.py`
are untouched, since they never wanted the second path at all. The console
UI's peer form (`console_web/mesh.html`) gains one more optional input.

### 7.5 Sequencing gap: the migration tool cannot read a live-written file safely

§5 did not say the source `vaino.db` must be static while the split runs.
Opening it through `sqlite3` (not a raw file copy) already handles WAL
correctly — committed frames included, uncommitted ones invisible, exactly
as a normal reader sees them — but "safely readable" and "a consistent
snapshot to build a replacement from" are different guarantees. If `vaino`
is still running and writing during the split, the two new files can
legitimately each be correct as of slightly different moments, and the
listener state committed in between is simply gone from both.

**Fix, folded into the live-migration runbook (§8) rather than left
implicit:** the split only ever runs with `vaino` and `mpd` stopped on the
target machine. Order: stop services → run the split tool against the now-
static file → verify → point the unit's arguments at the two new files →
start services → verify again. A rehearsal against a *downloaded copy* of
the live file (§8) has no such requirement, since nothing there is being
promoted to production.

### 7.6 Gap, found only by checking that the schema still works for `bose` and local: `PlayerStore`'s bootstrap can't just disappear

Asked directly whether the resulting schema stays equally functional on
`bose` and the local dev instance, not only vainopi — checking that
question against §4.3's own resolution found it had overreached. "Move
this table/column creation out of runtime `open()` entirely" would have
stopped `PlayerStore` from ever creating `file_tags`/`cover_art`/
`release_recordings`'s tuning columns **even on `bose` and local, which
are not splitting and whose `PlayerStore` connection genuinely still owns
that data.** That is exactly the fresh-library bootstrap failure the
original doc comment ("a library whose scan was already complete never
reached it and browsing died on a missing column") existed to prevent —
the review would have reintroduced the bug it was quoting as the reason
not to.

**Fixed, per `[§4.3]`'s correction above:** the bootstrap stays,
unconditionally, for the case that matters unchanged today — `alias ==
"main"`. It is skipped only when `alias == "lib"`, i.e. only on an
installation that has actually split, because in that case `PlayerStore`
has no writable path to run it against at all.

That still leaves a real gap for the split case alone: `tagscan.rs` opens
`library.db` via `Library::open_writable` and assumes `file_tags` and its
two indexes already exist — true today only because `PlayerStore` created
them first. **Checked, not assumed:** `tools/fetch_cover_art.py` already
creates its own `cover_art` table (`CREATE TABLE IF NOT EXISTS`) before
using it — already self-sufficient, needs no change. `tagscan.rs` does
not have the equivalent for `file_tags` and needs it added, so a
split-native `library.db` — one built by Sampo/`tagscan` directly rather
than descended from a single-file installation's split — bootstraps
correctly too, not only one that inherited the tables from a
pre-existing `PlayerStore` run.

### 7.7 Found in the real schema, not the source: a foreign key does cross the boundary after all

`[§7.2]` checked `player/src/db/mod.rs`'s and `player_store.rs`'s
in-tree `CREATE TABLE` text for a `REFERENCES` crossing the split and found
none. Checking the *actual deployed schema* on all three environments
directly — `sqlite_master`, not the Rust source — for this pass found one:

```sql
-- local, bose, and vainopi all agree, byte-for-byte:
CREATE TABLE listener_play_history (
    ...
    passage_id  INTEGER REFERENCES passages(passage_id) ON DELETE SET NULL,
    -- denormalised on purpose: six years of history must survive a rescan
    -- that renumbers passages [SPEC-SC-095]
    ...
```

`listener_play_history` (C) references `passages` (B) — a real
cross-boundary foreign key, present identically on every installation
checked. It is not in the current `PLAY_TABLE` constant in
`player_store.rs`, which defines `passage_id` with no `REFERENCES` clause
at all: the deployed databases carry a constraint an earlier version of
this file's schema added and a later simplification of the source stopped
re-stating — `CREATE TABLE IF NOT EXISTS` never retroactively strips a
constraint from a table that already exists, so the two have quietly
disagreed since whenever that simplification landed, on every
installation, unnoticed until this check went to `sqlite_master` instead
of the source. This is direct confirmation that `[§7.1]`'s migration tool
must keep copying DDL from the live database rather than reconstructing it
from the Rust constants, which are demonstrably not the same thing today.

**A second one, found later while enumerating every real table for
`tools/split_database.py` (§8) rather than only the ones already named
here:** `selection_decisions` (C) carries two — `passage_id REFERENCES
passages(passage_id)` (B) and `program_id REFERENCES
listener_programs(program_id)` (C, so unaffected). Same shape, same
conclusion below: nothing in the player enables `foreign_keys`, so this
has never been enforced from playback, split or not.

**Whether this matters depends on who's asking, and the answer differs by
environment:**

- SQLite does not enforce a `FOREIGN KEY` across two `ATTACH`ed databases.
  Once `passages` and `listener_play_history` are in different files, this
  constraint's `ON DELETE SET NULL` cascade simply stops firing — silently,
  since a query naming a schema-qualified table across an attach boundary
  doesn't error, it just doesn't get FK enforcement it would have gotten
  in one file.
- The player's own connections never `PRAGMA foreign_keys = ON`
  (checked — zero occurrences in `player/src/`), so this cascade has never
  fired from anything the player itself does, split or not.
- It **is** live today on the desktop: `segment_dao.py`, `apply_changes.py`,
  `apply_reviews.py`, `ingest_cd.py`, `ingest_folder.py`,
  `backfill_album_cuts.py`, `apply_boundary_reviews.py`, and
  `accept_remote_basis.py` all set `PRAGMA foreign_keys = ON`, and all run
  against the local desktop database when Sampo renumbers or deletes a
  passage — exactly the six-years-of-history case the inline comment
  names. **Neither `bose` nor `vainopi` ever runs any of these tools
  against its own database** — segmentation labor is desktop-only per
  `[SPEC035]`'s own decision that "Sampo work only happens on Sampo-capable
  nodes."

**Conclusion: does not block vainopi's migration.** vainopi never
exercises this cascade today and won't after splitting, for the same
reason it doesn't today — it has no Sampo. It also does not affect
`bose`, which isn't splitting. **It is a real, tracked prerequisite for
ever splitting the *local* database specifically** — before that could
happen safely, the eight tools above need an explicit application-level
replacement for the cascade (an `UPDATE listener_play_history SET
passage_id = NULL WHERE passage_id NOT IN (SELECT passage_id FROM
passages)`-shaped step run by whichever of them deletes or renumbers a
passage), matching the pattern `backup.rs::restore()` already uses for
cross-database consistency it can't get from a database-level constraint.
Not built, not needed for the work this document scopes, named here so it
is a known prerequisite rather than a surprise when local's own split is
eventually considered.

---

## 8. The migration procedure, corrected

Supersedes §5's sketch with the fixes from §7.

**`tools/split_database.py`**, same rehearse-by-default shape:

```
python tools/split_database.py vaino.db --library-out library.db --listener-out listener.db
python tools/split_database.py vaino.db --library-out library.db --listener-out listener.db --commit
```

1. Open the source read-only.
2. For each table in §3's B column: read its `CREATE TABLE` statement and
   every associated `CREATE INDEX`/`CREATE TRIGGER` statement from
   `sqlite_master`, execute them verbatim against `library.db`, then
   `INSERT INTO library.<table> SELECT * FROM source.<table>` — DDL first,
   data second, per `[§7.1]`. Same procedure against `listener.db` for the
   C column.
3. Verify before anything is promoted: row counts match table-for-table;
   `PRAGMA integrity_check` passes on both new files; spot-check primary
   keys; confirm every index named in the source's `sqlite_master` exists
   in the corresponding new file (not just that *a* index exists).
4. Report only, unless `--commit`; `--commit` is the only mode that writes
   to the real output paths rather than a temp directory.
5. Never deletes or modifies the source file.

**Live runbook against vainopi**, once the player refactor (§7.3's
`attach_library` helper and the ~10 rewritten call sites) has its own full
test cycle behind it and the tool above has been rehearsed against a
downloaded copy of vainopi's actual database (not synthetic data, not
bose's):

1. Off-device backup of vainopi's current `vaino.db`, per `[PI-C-030]` —
   mandatory, not conditional on anything else in this list.
2. Stop `vaino` and `mpd` on vainopi.
3. Copy `vaino.db` aside on vainopi itself (timestamped, not deleted —
   `[IMPL-DBSPLIT-045]`).
4. Run `split_database.py --commit` against that copy, producing
   `library.db` on B and `listener.db` on C.
5. Verify (step 3 of the tool's own procedure) before touching the running
   configuration at all.
6. Update the service unit's arguments to the two new paths.
7. Start `vaino` and `mpd`; verify both active, and verify audibly that
   playback still works — the same "heard it play" discipline
   `[IMPL-BOS-120]` already requires before any lock-in-style point of
   no return.
8. Update `sync_peers`/`remote_config` for vainopi's entry (§7.4) so
   `sync_preferences.py`/`remote_flags.py`/`export_flags.py` keep working
   from whichever machine drives them against vainopi.

Rollback at any point through step 6: stop services, point the unit's
arguments back at the single original `vaino.db` copy from step 3, start
services — no binary change needed, per `[§7.3]`.

---

## 9. Architectural review: indexed performance, and one more footgun found

Asked directly whether the split matches or beats today's query performance,
and whether the design is clean rather than merely correct. Checked both
against the real, identified call sites rather than against SQLite's
documentation in the abstract.

**Performance: measured, not assumed.** Every real cross-boundary reference
found in `[§2]`/`[§7]` is either a correlated scalar subquery/`EXISTS` check
(`PLAYS_EXPR`, `DESCRIBE`, the `listener_flags` check in `play_history`) or
independent single-schema queries combined by `UNION ALL`/`CASE` in SQL or by
a `HashMap` in Rust (`director/library.rs::load_taste`) — **never a bare
`JOIN` across the boundary at library scale.** That shape matters: a
correlated subquery's per-row cost is one indexed B-tree seek, and a B-tree's
seek cost does not change based on which file backs it. Built a real
two-file split of `browse_artists`' actual query shape against this
project's own 8,185 recordings plus 50,000 synthetic play-history rows and
ran `EXPLAIN QUERY PLAN` on both:

```
split:  SEARCH h USING COVERING INDEX listener_play_mbid (mbid=?)   -- 10.7ms / 8,185 rows
single: SEARCH h USING COVERING INDEX listener_play_mbid (mbid=?)   -- 10.1ms / 8,185 rows
```

**Identical query plan, indistinguishable timing** (the ~0.6ms difference is
noise at this scale). The split costs nothing here because the query shapes
already avoid the one pattern that would cost something — a real
constraint on future query design, not just a property of today's code:
**a bulk `JOIN` between a catalog table and a listener table, at full-library
scale, is the thing to avoid post-split** — not because SQLite can't use
indexes across an attach (it can, and does, as measured), but because a true
join needs the optimizer to choose a join order across two schemas, which is
less exercised territory than the correlated-subquery shape every current
call site already uses. Recommend this measurement become a real Rust
integration test in Phase 2 (rusqlite can run `EXPLAIN QUERY PLAN` directly),
so a future query that reintroduces a bulk cross-boundary join fails a test
rather than surfacing as a slow page someone eventually notices.

**Cleanliness: one more real footgun found, plus three checked and cleared.**

- **Found:** `[§4.3]`/`[§7.6]`'s conditional bootstrap (`PlayerStore` creates
  B-side tables only when `alias == "main"`) is correct but, written as a
  scattered `if` around each `CREATE`/`ALTER`, is exactly the kind of thing a
  later change forgets to guard. **Fix, before Phase 2 writes any of it:**
  one function, `ensure_library_tables_if_owned(conn, alias)`, called once,
  holding every one of those statements and the single guard — matching
  `attach_library`'s own "one place, not scattered" discipline rather than
  introducing a second kind of scatter to replace the first.
- **Checked, clear: no `CREATE TRIGGER` anywhere in this schema** (confirmed
  by the same grep that found the indexes in `[§7.1]`). Worth stating as a
  standing constraint, not just an absence: SQLite triggers cannot reference
  objects in a different attached database, so this split forecloses ever
  writing one that reacts to a catalog-table change by updating a
  listener-side column, or vice versa. Nothing today wants that; a future
  design should know it isn't available before reaching for it.
- **Checked, clear: no new denormalization.** The one column that already
  duplicates identity across the boundary — `listener_play_history.mbid`,
  alongside the FK-linked `passage_id` `[§7.7]` — predates this design
  entirely and exists for its own documented reason (surviving a rescan).
  The split adds no new duplicated column anywhere.
- **Checked, clear: the two-path peer model's failure mode is loud, not
  silent.** `[§7.4]`'s `remote`/`remote_listener` split means a
  misconfigured peer (wrong path in the wrong field) fails with "no such
  table" the first time a job runs against it, not a quiet wrong-answer —
  `mesh_diff`'s and `sync_preferences`' table names are disjoint by
  construction, so there's no name that resolves to the wrong file's table
  and returns a plausible-looking wrong result instead of an error.

**Verdict: solid.** Three review passes plus this one found six real
problems and fixed all of them before any of the migration itself ran;
this pass found a seventh (the bootstrap centralization) and confirmed
performance empirically rather than by assumption. Nothing found across
four passes changes the conclusion that the design delivers its intended
functionality — write-safety and backup-size improvements per `[§4.4]` —
at no performance cost, provided Phase 2 keeps every future catalog↔listener
query in the correlated-subquery shape already established rather than
introducing a bulk join.

---

## 10. Status

**Designed in detail, reviewed four times** — against synchronization,
RAM, backup, and MPD (§4); against the plan itself for gaps, oversights,
conflicts, and ambiguities (§7.1–7.5); against whether the resulting
schema stays equally functional on `bose` and the local instance, not only
vainopi (§7.6–7.7); and against indexed query performance and
architectural cleanliness (§9). Seven real problems found across the
second through fourth passes and fixed on paper before any of this was
built: a schema-fidelity gap in the migration tool, a compatibility
conflict that would have broken `bose`, a gap in the `tools/` peer-path
model affecting three existing scripts, a sequencing requirement the
original sketch left implicit, an overcorrection in `[§4.3]`'s own fix
that would have broken `bose`/local's fresh-library bootstrap, a real
cross-boundary foreign key present in every deployed database checked but
absent from the current Rust source — found by querying `sqlite_master`
directly rather than trusting the source, confirmed to not block vainopi
or `bose` while being named as a genuine prerequisite for ever splitting
*local* specifically — and a bootstrap-guard centralization the
performance/cleanliness pass caught before Phase 2 could scatter it.

**Performance confirmed, not assumed**: a real two-file split of this
project's own 8,185 recordings plus 50,000 synthetic plays, `EXPLAIN QUERY
PLAN`'d against both the split and single-file shapes, produced the
identical plan and indistinguishable timing (§9). The design costs nothing
because every real call site already uses correlated subqueries rather
than a bulk cross-boundary join — now a documented constraint on future
query design, not just an incidental property of today's code.

**Confirmed equally functional across all three environments for the work
this document actually scopes**: the `attach_library` alias approach
(`[§7.3]`) makes the one player binary behave identically on `bose` and
local (nothing attached, `alias = "main"`, unchanged) and on a split
vainopi (`lib` attached read-only) — no environment-specific binary, no
behavior change for the two that aren't splitting. The one open item
(`[§7.7]`) is scoped to a future local split, not to anything being built
or deployed now.

## 11. Player refactor: built and proven

**`attach_library()` and `QualifyingConn`** (`db/mod.rs`): built exactly as
`[§7.3]` specified — `"main"` when the two paths are equal, an attached
read-only `"lib"` only when they genuinely differ, one shared
`prepare`/`query_row`/`execute`/`execute_batch` rewrite point so every other
`Connection` method reaches through `Deref` unchanged. TDD'd against the
real linked SQLite before anything used it: same-path, different-path
(read-only enforced), two independent connections attaching the same file.

**`Library` and `PlayerStore`** both now hold a `QualifyingConn`. Every real
catalog-table reference found in `[§2]`/`[§7.6]` — 58+ sites in
`library.rs`, 8 in `player_store.rs`, plus `Director`/`FlavorIndex`'s own —
qualified with the `__LIB__.` placeholder and proven to resolve correctly
under both modes. `ensure_library_tables_if_owned()` built exactly as
`[§9]`'s cleanliness pass specified: one function, one guard
(`lib_alias() == "main"`), not scattered conditionals. `Library::open_writable`
made self-sufficient (`[§7.6]`) — it now calls `ensure_tag_table()` itself.

**A real end-to-end safety net, not just unit tests**: `library.rs` gained
an integration test that physically splits the existing `historyable()`
fixture across two real files — copying each table's actual DDL from
`sqlite_master`, not a second hand-written copy that could drift — opens
`Library::open_split` against them, and reruns `play_history`'s exact
assertions. It passes: title, artist, and album all resolve correctly
through the attached schema. This is the test that would catch a future
missed `__LIB__` prefix as a loud failure, not a slow page discovered by
a person.

Full regression throughout: 385 lib tests, all integration tests, the
whole workspace (every bin target) — green at each step.

## 12. The core playback path: wired

`bin/vaino.rs` (confirmed against `bose`'s actual systemd invocation to be
the real production binary — `mpd_direct.rs` is a dev tool) gained a
`--library` flag, defaulting to the `--db` path like every other split-aware
entry point. Threaded to all three places that used to clone `db` alone:
the tag-scan thread now scans `library` (`file_tags` is B-side), the engine
thread carries both into `Session::open`, and the backup thread was checked
and left on `db` alone — `backup.rs`'s own `LISTENER_TABLES` constant and
its doc comment ("Deliberately NOT here: `files`, `passages`, `recordings`,
...") confirm backup is already listener-only by design, needing no second
path. `Session` itself now holds both paths, including in its
rebuild-in-flight closure (`[IMPL-SUI-075]`) — the one place still calling
the single-path `Library::open` after everything else was converted.

**A real, larger-than-expected gap found while wiring this, not assumed
away**: `web::Ui` gained a `library` field, but its ~23 existing handler
call sites — across `browse.rs`, `edit.rs`, `review.rs`, `media.rs`,
`segment.rs`, `preference.rs`, `control.rs`, `bluetooth.rs`, `sampo.rs` —
all still open only `ui.db.clone()` and call `Library::open`/`PlayerStore::open`
with that one path. On unsplit `bose`/local this is invisible, since `open`
already defaults to same-path. **On a split vainopi, every one of those
pages fails with "no such table" the first time it's opened**, because
`Library::open(db)` becomes `open_split(db, db)` and `attach_library` sees
the two paths equal, resolving catalog references against `main` —
`listener.db` — which does not have them.

This is real remaining work, scoped out of this pass deliberately rather
than rushed: converting 23 call sites across 9 files, each needing a second
captured path threaded into an existing `tokio::task::spawn_blocking`
closure, deserves its own focused pass with the same care the qualified
queries got — not a mechanical sweep squeezed into the tail of this one
with no test coverage of the web handlers' actual database behavior to
catch a mistake. **The core playback path — audio, resume state, tag
scanning, listener backup, radio selection, its periodic rebuild — is fully
split-aware and proven. Vainopi's web UI (browse/edit/review/segment
pages) is not, yet, and must not be assumed to work until this is done.**

Full regression at every step: 385 lib tests, all integration tests, the
whole workspace (every bin target) — green.

## 13. The web layer: closed

All 23 call sites (§12) now clone `ui.library` alongside `ui.db` and open
through `open_split`. Verified by a clean compile (an unused `library`
binding would have warned) and full regression; not further tested at the
handler/HTTP level, since this is purely mechanical threading with no new
SQL — the qualification logic itself is proven at the `Library`/`PlayerStore`
level already. `sampo.rs`'s one remaining `ui.db` use launches Sampo's own
console as a subprocess and is out of scope: Sampo runs on the desktop,
which stays single-file regardless of this work.

## 14. `tools/split_database.py`: built and run against real data

Built exactly to `[§7.1]`'s corrected procedure — every table's real DDL
(`CREATE TABLE` and every associated `CREATE INDEX`) read from the source's
own `sqlite_master` and replayed before `INSERT INTO ... SELECT`, never
`CREATE TABLE ... AS SELECT`. Rehearses by default; `--commit` is the only
mode that writes the real output paths, and refuses outright if either
already exists rather than overwriting. Never opens the source for
anything but reading.

**Enumerating every real table for this** (rather than only the ones
`[§2]`/`[§7.6]` had already named) found two more things worth recording
plainly:

- A second cross-boundary foreign key, `selection_decisions.passage_id →
  passages` — folded into `[§7.7]`'s finding rather than treated as new,
  since the conclusion is identical.
- `schema_meta` (two rows: `schema_version`/`spec`) describes *a file's*
  schema, which is ambiguous the moment there are two files. Judgment
  call, stated as one rather than silently resolved: copied to **both**
  outputs, so a future tool checking a database's schema version finds it
  regardless of which half it opened.

**Tested two ways**: `tools/test_split_database.py` (rehearsal writes
nothing, `--commit` produces two correct files with their indexes intact
and leaves the source untouched, refuses to overwrite an existing output,
tolerates a source missing an optional table) — all pass. Then rehearsed
for real against this project's own `data/vaino_new.db` — the same file
that seeded `bose` — with no synthetic substitute: **1,066,520 rows across
19 catalog tables, 42,321 rows across 15 listener tables, verification
passed**, and confirmed nothing was written to disk, per the rehearsal
contract.

## 16. `sync_peers`'s `remote_listener`: the registry half is done, the query half is not

Built: the additive `ALTER TABLE sync_peers ADD COLUMN remote_listener` migration
(guarded against re-running on a sidecar that already has it), `remote_config`'s
equivalent `sync_remote_listener` key, and `get_remote_listener()`/
`set_remote_listener()`/`list_peers()`/`upsert_peer()`/`activate_peer()` all
carrying the second path through — `NULL` means "same file as `remote`",
true for every peer that hasn't split, so nothing already configured needs
re-entering. `console.py`'s `/api/peers` route and `mesh.html`'s peer form
both accept an optional second address, blank by default. Six tests in
`test_jobs_peers.py` cover the default-to-`None` case, carrying a real
second path through `upsert`→`activate`→`get_remote_listener()`, and
switching from a split peer back to an unsplit one correctly clearing the
stale listener path rather than leaving it active against the wrong peer.

**Not done: the three tools this exists for don't consume it yet.**
`remote_flags.py`'s `FLAGS_SQL` is one query joining `listener_flags` (C)
against `passages`/`files` (B) in a single `sqlite3` invocation over `ssh`
— exactly the shape that cannot run against a split peer's single file, the
reason `remote_listener` exists at all. Making it work needs `remote_peek.py`'s
`run_remote_sql()` to optionally `ATTACH` a second path before running the
query (the same "open listener as main, attach library as `lib`" direction
`[PI-DB-020]` and `attach_library()` already use in Rust), and `FLAGS_SQL`'s
catalog references qualified with `lib.` to match. `sync_preferences.py`
needs the identical treatment for its own existence-check join. `export_flags.py`
runs locally against `self.library` already and needs only to also open
`self.library`'s listener-side file if that ever differs — the smallest of
the three, and the only one not making a remote SQL call at all. None of
this is built; the registry can store and hand back a listener path today,
but nothing yet asks for one.

## 17. Rehearsed against vainopi's actual data, not a substitute

Before ever touching the live device: cross-compiled `vaino` for `aarch64`
(confirmed genuine — `file` reports `ELF 64-bit ... ARM aarch64`), pulled a
read-only copy of vainopi's real `/srv/library/vaino.db` to the dev host
(1,161,781,248 bytes, matching exactly — this copy also satisfies `[PI-C-030]`'s
off-device backup requirement for the live runbook below, so it's being kept,
not discarded), and rehearsed `split_database.py` against it: 1,065,361
catalog rows across 19 tables, 44,337 listener rows across 15, verification
passed. Then committed the split for real to scratch files and proved it
two ways against actual Rust code, not just the Python tool's own checks:

- `flavorcheck` against the resulting `library.db` alone: 8,151 flavor
  subjects loaded correctly.
- `dircheck` (given an optional second path for exactly this — `[§17]`'s
  own addition) against the real `listener.db`+`library.db` pair:
  **`Director::load` succeeded** — the one function reading
  `listener_play_history`/`listener_likes`/`listener_preferences`/
  `listener_programs` from one file and `recordings`/`artists`/`flavor`/
  `recording_relations`/`passages`/`files` from the other, and the most
  consequential cross-boundary path there is, since it drives actual song
  selection. 8,330 radio passages, 733 ms cold load, 148 MB peak RSS —
  comfortably inside `[REQ-HW-100]`'s budget even before accounting for
  this being vainopi's tighter 464 MB, not the dev host's.

This is real-data proof, not synthetic-fixture proof, for the one thing
that most needed it.

## 18. The live migration: run, and one real bug found and fixed inside the same window

Executed against vainopi for real, 2026-09-07, following `[§8]`'s runbook
exactly: stopped `vaino`/`mpd`, confirmed the WAL was already clean (the
service's own shutdown had checkpointed it — `PRAGMA wal_checkpoint(TRUNCATE)`
afterward reported zero pages, confirming rather than assuming), made a
timestamped on-device copy (`vaino.db.pre-split-20260907`, kept, not
deleted), ran `split_database.py --commit` against that copy on vainopi's
own hardware (1,065,361 catalog rows, 44,337 listener rows — identical to
the dev-host rehearsal, verification passed), deployed the cross-compiled
binary, updated the systemd override's `ExecStart` to the two new paths,
reloaded, and restarted.

**`[IMPL-DBSPLIT-055]` Found within the first minute, from the journal, not
from `systemctl is-active` alone:** `record decision: query: no such table:
main.passages` and `save player state: query: no such table: main.passages`,
on every single call. Root cause: `[§7.7]`'s own review had already found
both cross-boundary foreign keys (`listener_play_history`/
`selection_decisions` → `passages`) but concluded they were inert because
nothing in this crate explicitly enables `PRAGMA foreign_keys` — true, and
the wrong question. Never checked whether **rusqlite's bundled SQLite
defaults it to ON**, which it does, confirmed by asking it directly.
SQLite only ever checks a `FOREIGN KEY` against a table in its own schema,
never an attached one — so every write to a table referencing the
now-attached `passages` failed, silently (both call sites are deliberately
non-fatal — "must never stop the music" — so audio kept playing throughout,
which is exactly why this needed the log, not the service status, to find).

Same discipline as `bose`'s `[IMPL-BOS-165]`/`[IMPL-BOS-166]`: found live,
root-caused, fixed, tested, and redeployed inside the same incident window
rather than left running degraded. Fix: one `PRAGMA foreign_keys = OFF` in
`QualifyingConn::open()`, turned off unconditionally rather than only when
split, since nothing in this crate ever relied on the cascade firing from
the player's own writes. TDD — a new test reproduces the exact failure
shape and confirms the fix — plus full regression (386 tests), before
redeploying. Confirmed fixed against the live system two ways: the journal
stopped showing the error, and `listener.db` queried directly showed fresh
`player_state`/`selection_decisions` rows landing in real time.

**Status: `vaino` and `mpd` both active on vainopi, running the split
database, writes confirmed landing correctly.** Not yet confirmed: audible
playback, the one verification this project has never allowed a machine or
a log line to stand in for at a point of no easy return.

## 19. What remains

- **The three tools' query-level ATTACH support** (§16) — real, scoped,
  understood, not yet built. Does not block the migration — mesh-sync
  catalog diffing already works against a split peer per `[§4.1]`; only
  `remote_flags.py`/`sync_preferences.py`'s own cross-table fetch is
  affected, and neither runs unattended.
- **Hearing it play** — the last step, and the only one that has never
  been substitutable by a green test or an active service.

Scope for the first real implementation and migration pass stays vainopi
only; `bose` and local stay single-file and untouched until vainopi has
proven the split in practice.
