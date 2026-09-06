# SPEC035: Mesh Synchronization of Library Content

**Design Specification — Tier 2 · Not yet built**

The fifth deployment topology [SPEC006 §6](SPEC006-data-flow-and-portability.md#6-deployment-topologies)
doesn't name: two or more **Sampo-capable** installations, each independently
ingesting, that want the union of their catalogs — audio, identification,
segmentation, flags — without ever reconciling play history or likes. Decided
against a concrete fourth node: a PC-based Vaino/Sampo instance (`TeachersLounge`,
an Ubuntu laptop) joining a family that already has a desktop, `vainopi`, and
`bose`.

> **Related:** [SPEC006](SPEC006-data-flow-and-portability.md) §§2–6 — the
> identity model and class system this extends, not replaces · [SPEC013 §5](SPEC013-sampo-console.md#5-export--new-music-to-a-remote-vaino)
> — the bundle exporter/importer this reuses as its payload mechanism ·
> [SPEC022](SPEC022-flag-and-edit-sync.md) — the review/flag sync this
> generalizes past one hub-and-spoke pair · [SPEC030](SPEC030-preference-sync.md)
> — the precedent for a narrow, table-scoped exception to Class D

---

## 1. What's already true, restated so this document doesn't relitigate it

**`[SPEC-MESH-005]`** Nothing here changes [SPEC006 §3](SPEC006-data-flow-and-portability.md#3-what-travels-and-what-must-not)'s
class system. Class A (derived facts), B (identification) and C
(segmentation) travel; Class D (play history, likes, preferences, programs)
never does, except `listener_flags`, which [SPEC022](SPEC022-flag-and-edit-sync.md)
already carved out by portable anchor rather than by relaxing the rule. Asked
directly for this design: "share all music files and metadata definitions
including flagging, but not play history and likes" is not a new policy —
it is Class A/B/C-plus-flags, Class D-minus-flags, the system that already
exists, applied to more than two nodes.

**`[SPEC-MESH-010]`** Nothing here replaces the bundle mechanism
([SPEC013 §5](SPEC013-sampo-console.md#5-export--new-music-to-a-remote-vaino)).
A bundle — audio plus the class A/B/C payload for hashes the target lacks,
Class D excluded by construction — is still the unit that moves. What this
document adds is *how a set of hashes to bundle gets decided*, across more
than one direction and more than one peer, instead of a human typing
`--like`/`--md5` for one known addition.

---

## 2. The topology

**`[SPEC-MESH-020]` A fifth row for [SPEC006 §6](SPEC006-data-flow-and-portability.md#6-deployment-topologies)'s table:**

| Topology | Flow |
| :--- | :--- |
| **Mesh** (N Sampo-capable peers) | Each ingests independently. Any pair periodically computes an automatic diff, a person reviews it, approved items move as bundles in whichever direction closes the gap — never both directions blindly, never unattended. |

This is not "Appliance" with more nodes: Appliance's whole design rests on
one side never running Sampo `[GDE-ARC-010]`, `[SPEC-DF-108]`, which is
exactly what makes its trust rule ("desktop's claim wins") sound. A Mesh peer
can originate a manual correction just as validly as the desktop can — see
§6 for what that breaks and how it's closed.

**`[SPEC-MESH-025]` Membership is a short, explicit, human-maintained list —
not discovery.** Four nodes today; this does not need to scale past what a
person can name. `remote_config` (`[SPEC022]` §2) currently stores exactly
one remembered remote per library — extended here to a small table,
`sync_peers(name, remote, enabled)`, one row per known peer
(`desktop`/`vainopi`/`bose`/`teacherslounge`, each an `ssh` `user@host:/path`
per `[SPEC-DF-116]`'s existing addressing). A peer is added or removed by
editing one row; nothing here builds peer discovery, health-checking, or
auto-registration.

---

## 3. Automatic diffing

**`[SPEC-MESH-030]` The diff is key-and-fingerprint, not a database copy —
`[SPEC-DF-119]`'s precedent generalized from one small table to several
larger ones.** For each identity-scoped table that matters to A/B/C —
`files` (by `audio_md5`), `recordings`/`artists`/`releases` (by `mbid`),
`passages`/`passage_recordings` (by `(audio_md5, kind, start_ms, end_ms)`)
— one remote `SELECT` returns every key plus the columns a person would
actually need to judge a difference (title, artist credit, boundary values,
`source`/`boundary_src`), **never** `lowlevel_cache`, `musicbrainz_cache`, or
`identification_cache` — the 73% of a library's bytes `[SPEC-SUI-090]`
already found no reason to move. Keys and a handful of short columns, not
gigabytes: the same order of cost `[SPEC-DF-119]` measured for
`listener_flags`, applied to tables that are larger but still nowhere near
the full database.

**`[SPEC-MESH-035]` The comparison runs locally, in memory, against the
requesting side's own tables — no new remote round trip per key.** Two
`SELECT`s (local, remote) become one classification pass:

| Bucket | Meaning |
| :--- | :--- |
| **local-only** | this side has it, the peer doesn't — a bundle candidate, this side → peer |
| **peer-only** | the peer has it, this side doesn't — a bundle candidate, peer → this side |
| **agree** | present on both, values match — nothing to do |
| **conflict** | present on both, values differ, at least one side's provenance is `manual` | see §6 |

`[SPEC-MESH-036]` A both-computed, non-`manual` disagreement (two peers'
Sampo independently identified or segmented the same audio differently, no
person involved on either side) is **not** a conflict requiring a human —
it resolves by `[SPEC-DF-070]`'s existing rank-then-recency rule
mechanically, the same way an import already would. §6 is only for the case
neither `[SPEC-DF-070]` nor `[SPEC-DF-105]`'s three-way merge was ever
built to arbitrate: two *people*, on two peers, having each already decided.

**`[SPEC-MESH-038]` This produces a diff, and stops.** `[SPEC-MESH-030..036]`
is read-only against both sides. No row moves as a result of running it.

---

## 4. The human review gate

**`[SPEC-MESH-040]` The diff is a proposal; a person decides what actually
crosses.** Explicit design constraint, not a missing feature: `[SPEC-MESH-030]`'s
output is a report — reused directly as `export_bundle.py`'s existing
`--md5`-list input (`export_bundle.py` already accepts an appended list; this
adds reading one from a file rather than typing each on the command line) —
never an auto-apply. A person looking at "peer-only: 214 recordings, 340
passages" can exclude an in-progress album someone hasn't finished
segmenting, or a folder that was only ever meant to stay local. This is the
same discover-then-let-a-person-confirm shape [SPEC-SUI-215]'s release
suggestion already uses, applied to the mesh diff instead of one release
candidate.

**`[SPEC-MESH-045]` Review happens once per diff, not once per row.** The
report groups by album/artist where the schema already supports it
(`release_recordings.disc`/`position`), so approving "the Frisina addition"
is one decision even though it touches dozens of passage rows underneath —
matching `[SPEC-SUI-090]`'s own framing of the common case as "six files,"
never a spreadsheet of individual `audio_md5`s to check one by one.

**`[SPEC-MESH-047]` Reached from the console, the same job model as
everything else.** `jobs.py` gains a `mesh-diff` kind (runs `[SPEC-MESH-030]`
against a named `sync_peers` row, reports the four buckets) and the existing
"Sync with a remote" section grows a peer selector reading `sync_peers`
instead of the single `remote_config` value — `remote-pull`/`remote-push`/
`sync-preferences` keep working exactly as they do today against whichever
peer is selected; nothing about their own mechanism changes.

---

## 5. Segmentation labor stays where Sampo runs, and duplication is accepted

**`[SPEC-MESH-050]` Class C is computed only on a node that has ingested the
audio through Sampo — never inferred, borrowed, or transplanted across a
different encoding.** Decided directly, closing what would otherwise be an
open question: if `TeachersLounge` rips its own copy of an album the desktop
already segmented, its `audio_md5` differs, `[SPEC-MESH-030]`'s diff correctly
shows it as `local-only` on both sides, and `TeachersLounge`'s own Sampo
re-does the segmentation from scratch. No cross-encoding boundary-reuse
mechanism is designed or wanted — the redundant labor is accepted as a cost
of independent ingest, not a defect to engineer around. This is a **scope
decision**, not an oversight: `[SPEC-DF-040]`'s own scoping (Class C binds to
`audio_md5`, meaningless against a different rip) already says a shared
boundary would be a guess dressed as a fact, and this document declines to
build the guess.

**`[SPEC-MESH-052]` Class A/B for that same audio still converges, because it
was already designed to.** Once `TeachersLounge`'s rip resolves to the same
`recording_mbid` (via its own AcoustID lookup, independently), flavor and
identification match by construction (`[SPEC-DF-040]`'s recording scope) even
though the encodings, and therefore the Class C boundaries, never do. The
diff's `agree` bucket for `recordings` and the `local-only`/`peer-only`
buckets for `passages` on the *same* mbid are the visible signature of
exactly this: one recording, two legitimately separate sets of boundaries.

**`[SPEC-MESH-055]` No claim-before-ingest coordination.** Two peers ingesting
the same new album before ever syncing both pay the identification and
segmentation cost independently. Not solved, not attempted — `[SPEC-MESH-025]`'s
membership list is small and manually curated specifically because this
project has no present need for a coordination protocol at this scale.

---

## 6. Manual-vs-manual conflicts

**`[SPEC-MESH-060]` A conflict is: both sides have a value, and at least one
carries `source`/`boundary_src` = `manual`, and the values disagree.**
`[SPEC-DF-070]`'s existing rule — provenance rank, then recency — already
resolves *machine-vs-machine* and *machine-vs-manual* disagreements without a
person. What it does not resolve, and was never asked to, is **manual vs.
manual**: two people, on two different peers, each having corrected the same
recording's artist credit, or the same passage's boundary, differently,
before either side had seen the other's edit.

**`[SPEC-MESH-065]` This is an amendment to [SPEC006 §5](SPEC006-data-flow-and-portability.md#5-trust-and-conflict),
stated as one rather than silently reinterpreted.** §5's rule ("local
outranks imported at equal rank") was sound because only one side of any
existing sync could ever hold a manual opinion — the appliance never runs
Sampo, so "local" and "the desktop's own correction" were always the same
thing. That stops being true the moment a second Sampo-capable peer exists:
applied literally by both sides at once, "local wins" is symmetric and
**non-convergent** — each peer keeps its own value forever, which is the
opposite of "reasonably up to date with each other." The amendment: **when
both sides are `manual` and disagree, `[SPEC-DF-070]`'s rank-then-recency
rule does not apply — this is routed to a person instead of decided by
either side unilaterally.**

**`[SPEC-MESH-070]` Resolution is a value, not a flag, and it is applied to
*both* sides identically.** No new "conflict" table, no persisted
resolved/unresolved state. A person picks (or types) the winning value; that
exact value is written to whichever side(s) don't already hold it, with
`source`/`boundary_src` set to `manual` and a fresh timestamp, through the
same bundle/patch mechanism `[SPEC013 §5]` already uses. Once written, the
two sides hold **the same value with the same provenance rank** — a future
diff (`[SPEC-MESH-030]`) sees `agree`, not `conflict`, because the data
itself converged. This is `[SPEC-MESH-070]`'s answer to "once resolved,
should revert to unremarkable": there is nothing to revert, because nothing
was ever recorded as remarkable in the database — only the diff *report*
named it, and the next report has nothing left to name.

**`[SPEC-MESH-075]` Reached the same way `[SPEC-DF-116]`'s accept-remote-basis
flow already works, generalized from one anchor to a batch.** `§4`'s review
UI shows each conflict's two current values side by side (the shape
`accept_remote_basis.py` already established for one id/boundary at a time);
picking one, the other, or typing a third is the same interaction, run once
per conflict in the batch rather than requiring a separate page visit per
row.

---

## 7. A prerequisite this design depends on, resolved here

**`[SPEC-MESH-080]` Closes `[SPEC-SUI-180]` — "re-importing the same bundle is
unspecified." Built 2026-09-06, and turned out narrower than first
written here.** Reading `player/src/bundle.rs` before touching it found it
already more idempotent than `[SPEC-SUI-180]` assumed: `files.audio_md5` is
checked before any write, so a resent encoding already reports `Already` and
writes nothing, and `upsert_recording()` already carries `[SPEC-DF-070]`'s
provenance check for flavor. **The actual gap was narrower and sharper: an
already-held encoding's whole block was skipped, including its recordings'
credits — so a *later, different* bundle improving Class A/B data (a better
flavor value, most concretely) for audio already on disk was silently
dropped, not merged.** Fixed by running `upsert_recording()` for an
already-held encoding's credits too, gated on `apply`, touching no
`files`/`passages` rows. Proven by two tests added first and confirmed
failing against the unfixed code:
`a_later_bundle_still_updates_flavor_for_an_already_held_file` (the gap) and
`a_manual_flavor_value_survives_a_later_computed_bundle_even_when_the_file_is_already_held`
(provenance protection still holds on the new call site, not only the old
one). A second, smaller fix landed alongside it: `imported_payloads` was
appending an audit row on every resend even when nothing was written;
now gated on `rows_written > 0`.

**Left as found, out of scope for this fix specifically: `recordings.title`/
`length_ms`/`source` never update once a recording row exists, on *any*
call path, not only the one this fix touches.** `upsert_recording()`'s own
`exists` check skips the whole row, always — a pre-existing behavior, not
introduced or widened here. Worth its own pass (the same provenance check
`flavor` already gets, applied per-recording instead of per-characteristic),
but conflating it with `[SPEC-MESH-080]`'s narrower fix risked a larger,
riskier change than the gap actually found required.

**`[SPEC-MESH-085]` `[SPEC-SUI-175]`'s "nothing re-reads a retained payload"
is real but not a blocker here, and is left open deliberately.** A mesh peer
running an older Sampo/Vaino than the one that produced a bundle already
retains fields it can't interpret, per `[SPEC-SUI-165]`; failing to later
re-parse them after an upgrade is a missed optimization (a re-transfer would
fix it), not a correctness problem this design introduces or must solve to
proceed. Tracked at `[SPEC013 §6]`, unchanged by this document.

---

## 7a. Concrete design: the peer registry and the conflict review UI

**`[SPEC-MESH-090]` `sync_peers` lives in the console's own sidecar, beside
`jobs`/`remote_config` — never a table Vaino reads `[SPEC-SC-015]`, same
reasoning `[jobs.py]`'s own schema comment already gives for everything
else there.**

```sql
CREATE TABLE IF NOT EXISTS sync_peers (
    name    TEXT PRIMARY KEY,
    remote  TEXT NOT NULL,           -- user@host:/path/to/vaino.db
    enabled INTEGER NOT NULL DEFAULT 1
);
```

**`[SPEC-MESH-092]` Additive, not a replacement — `remote_config` keeps
working exactly as it does today.** `[SPEC022]`'s three sync jobs
(`remote-pull`/`remote-push`/`sync-preferences`) already read one address via
`get_remote()`, tested and live-verified; rewriting that to read `sync_peers`
directly would risk exactly the regression this design has no reason to
invite. Instead: **selecting a peer calls `set_remote(peer.remote)`**, so the
three existing jobs keep calling `get_remote()` unchanged and simply act on
whichever peer was selected last. `sync_peers` is where names live;
`remote_config` stays what those three jobs actually read.

**`[SPEC-MESH-094]` The API, mirroring `/api/remote`'s existing shape:**

| Route | Method | Does |
| :--- | :--- | :--- |
| `/api/peers` | GET | list `sync_peers` |
| `/api/peers` | POST | upsert one (`{name, remote}`) |
| `/api/peers/<name>` | DELETE | remove one |
| `/api/peers/<name>/activate` | POST | `set_remote(peer.remote)` — makes it the target for the three existing sync jobs |
| `/api/mesh/diff` | POST | `{peer}` → submits a `mesh-diff` job against that peer's `remote` |
| `/api/mesh/resolve` | POST | `{peer, table, key, choice}` → submits a `mesh-resolve` job, `choice` one of `"local"`/`"peer"` |

**`[SPEC-MESH-096]` `mesh-diff` is a job like any other** — `_mesh_diff(job_id,
target)` runs `mesh_diff.py <library> <target> --json` as one stage
(`_run_single_stage`, the same shape `sync-preferences` already uses), and
the diff's full report becomes the job's `result`. `mesh_diff.py` gained
`--json`: a final line `{"ok": true, ...report}` — found-a-conflict is data
in the result, never a job failure, since nothing was asked to act yet
`[SPEC-MESH-038]`.

**`[SPEC-MESH-098]` `mesh-resolve` performs `[SPEC-MESH-070]`'s write, to
whichever side(s) disagree with the chosen value.** `tools/resolve_mesh_conflict.py`
(new): given a table, an identity key, and a chosen side, it writes that
side's current value to the *other* side with `source`/`boundary_src`
forced to `manual` — locally through a direct connection, remotely through
the identical `sudo systemctl stop vaino && sqlite3 ... && sudo systemctl
start vaino` recipe `[push_file_tags.py]`/`[sync_preferences.py]` already
use, quoted with `remote_peek.literal()`. A person typing a third value
neither side has yet is `--value`, applied to both sides the same way. The
job kind (`_mesh_resolve`) is a `_run_single_stage` wrapper exactly like
`sync-preferences`'s.

**`[SPEC-MESH-100]` One new console page, `/mesh`, not a section bolted onto
`/flags`.** `/flags`'s "Sync with a remote" section is unchanged. `/mesh`
carries: the peer list (add/remove/activate), a "diff against" selector plus
button, and — once a diff has run — three panels per table: counts for
`local_only`/`peer_only` (each with an `export_bundle.py --md5-file`
command line, printed rather than automated further here — `[SPEC-SUI-110]`
already decided a bundle target is "an ssh host and a directory, never a
Vaino endpoint," and this document does not relitigate that), and a list of
`conflict` rows, each with the two values side by side and "use local"/"use
peer" buttons wired to `/api/mesh/resolve`.

---

## 8. What remains open after this document

1. ~~**`sync_peers` and the console's peer selector are designed, not built.**~~ **Built 2026-09-06** — §7a: `sync_peers` (additive, `remote_config` untouched), `Runner.list_peers/upsert_peer/delete_peer/activate_peer`, the `/api/peers*` routes, and a new `/mesh` console page with the peer list and a diff-and-resolve UI. Four test files (`test_jobs_peers.py`, `test_jobs_mesh_diff.py`, `test_jobs_mesh_resolve.py`, plus `resolve_mesh_conflict.py`'s own).
2. ~~**The diff tool is designed, not built.**~~ **Built 2026-09-06**: `tools/mesh_diff.py`, covering `files`/`recordings`/`passages` (the three tables §3 named), the four-bucket classification, and the manual-vs-manual conflict rule — five tests, `tools/test_mesh_diff.py`, the remote side faked the same way `test_remote_flags.py` already established, the local side a real temporary sqlite file. `artists`/`releases` are not yet added to `TABLES`, mechanically the same shape as `recordings` when they are.
3. ~~**`export_bundle.py` reading a file of hashes is a small addition, not built.**~~ **Built 2026-09-06**: `--md5-file`, two tests in `tools/test_export_bundle.py` (the tool's first test coverage of any kind — the new flag, not the pre-existing pipeline, is what's actually verified).
4. ~~**`import_bundle.rs`'s classify-before-write is not built.**~~ **Built 2026-09-06** — narrower than first written here; see `[SPEC-MESH-080]`'s own updated text for what the gap actually was.
5. ~~**The conflict review UI is not built.**~~ **Built 2026-09-06**: `tools/resolve_mesh_conflict.py` (the write, tested per `[SPEC-MESH-070]`'s decision logic the same way `test_sync_preferences.py` established — the actual `ssh`/`scp` leg verified live, not by unit test), the `mesh-resolve` job kind, and `/mesh`'s conflict table with "use local"/"use peer" buttons. Every table with a `manual_field` (`recordings`, `passages`) is covered; `files` carries no provenance and can never produce a conflict to resolve.

**What this pass did *not* build, deliberately:** `--value` (a third value neither side has yet) has a CLI and job-kind path but no UI — `/mesh` only ever offers "use local"/"use peer". A person wanting a genuinely new value still runs `resolve_mesh_conflict.py --value` by hand. The `local_only`/`peer_only` buckets print an `export_bundle.py` command rather than running one — `[SPEC-SUI-110]`'s "an ssh host and a directory, never a Vaino endpoint" stance, applied to the UI too.
6. **Storage-tier policy — must every peer hold every file? — is explicitly out of scope here.** Asked and not answered: nothing in this document decides whether `vainopi` (464 MB RAM, a small card) is expected to eventually hold the full union library. `[SPEC-MESH-040]`'s human review gate is the mitigation available today — a person can simply decline to approve a bundle a small node shouldn't receive — but "catalog knows about this recording, audio absent here" is not a state the schema represents, and a mesh that grows past hand-curated approval may need it to be. Deferred, not resolved.

---

**Traceability:** `[SPEC-MESH-005..085]` · derives `[REQ-PORT-160..200]` ·
extends [SPEC006](SPEC006-data-flow-and-portability.md) §§3, 5, 6 (§5's
amendment is `[SPEC-MESH-065]`) · reuses [SPEC013 §5](SPEC013-sampo-console.md#5-export--new-music-to-a-remote-vaino)'s
bundle mechanism and `apply_changes.py`'s `classify()` · reuses
`tools/remote_peek.py`'s `run_remote_sql()`/`literal()` · closes
`[SPEC-SUI-180]`, leaves `[SPEC-SUI-175]` open · addresses the gap named at
the end of the prior review of [SPEC006](SPEC006-data-flow-and-portability.md),
[SPEC013](SPEC013-sampo-console.md) and [SPEC022](SPEC022-flag-and-edit-sync.md)/[SPEC030](SPEC030-preference-sync.md) together
