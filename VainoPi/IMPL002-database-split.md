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
**`[IMPL-DBSPLIT-005]`** No change to either sync tool is needed for this.
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
bigger, but still bounded and enumerable. **`[IMPL-DBSPLIT-010]`** As of this
writing, `vaino` opens the *same* `vaino.db` path three separate times, for
three different reasons:

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

**Resolution: move this table/column creation out of runtime `open()`
entirely, into the migration tool itself (§4.4) and into `attended-import.sh`'s
own "prepare B for writing" step**, alongside whatever `tagscan` and
`fetch_cover_art.py` already do when they open B read-write. The player never
needs to create these — it only ever needs to find them already present,
the same way it already tolerates an absent `lyrics` table by treating the
query failure as "none" (`db/library.rs`'s `lyrics()` doc comment). A
library.db that predates this split, or predates a given tool, is a gap in
what's been run against it, not a fault the player papers over at startup.

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

## 9. Status

**Designed in detail, reviewed twice** — once against synchronization,
RAM, backup, and MPD (§4), and once against the plan itself for gaps,
oversights, conflicts, and ambiguities (§7). Four real problems found in
the second pass and fixed on paper before any of this was built: a schema-
fidelity gap in the migration tool, a compatibility conflict that would
have broken `bose`, a genuine gap in the `tools/` peer-path model
affecting three existing scripts, and a sequencing requirement the
original sketch left implicit. One thing checked and found *not* to be a
problem, stated rather than assumed: no foreign key crosses the boundary.

**Not yet built:** the player refactor, `tools/split_database.py`, the
`sync_peers` schema addition, and the live migration itself. None of these
have touched real data yet. This document is now the corrected plan;
executing it — starting with the `attach_library` helper, since everything
else depends on it — is the next concrete step.
