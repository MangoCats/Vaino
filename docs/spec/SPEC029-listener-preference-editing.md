# SPEC029: Editing an Artist's or a Recording's Own Rotation, Recovery, and Restraint

**Design Specification — Tier 2 · Built**

MuLibPlay let a listener hand-tune `rotation`/`recovery`/`restraint` per
artist and per recording directly — real, used functionality: `[GDE-BMK-020]`
records that 2,918 of 8,116 tracks (36%) carried a tuned value. Vaino's
schema (`listener_preferences`, [SPEC008 §6](SPEC008-database-schema.md#6-listener-state--class-d))
already carries those exact rows forward unchanged from the MuLibPlay
migration, but until this document nothing reached them except
`Director::load()` at startup and the one-time migration script — no route,
no page, anywhere, to view or change a single artist's or recording's own
tuning. This restores that surface.

> **Status.** Built 2026-09-04, per `[REQ-VIS-285]`, `[REQ-VIS-290]`.
> `player/src/web/preference.rs` (routes), `PlayerStore::get_preference`/
> `set_preference`/`reset_preference` (`player/src/db/player_store.rs`), and
> `Vaino.editPreference` (`player/src/web/core.js`, the one shared panel
> every skin below reuses).

> **Related:** [SPEC008 §6](SPEC008-database-schema.md#6-listener-state--class-d) for
> `listener_preferences`'s own schema · [SPEC009](SPEC009-program-director.md) supplies
> the log-scale formulas this panel's readouts use (`[SPEC-DIR-110]`/`[SPEC-DIR-115]`)
> and the master time-scale dials this document does not touch (`[SPEC-DIR-118]`) ·
> [GUIDE001](../GUIDE001-lineage-and-lessons.md) supplies the migration count
> establishing this was real, used functionality (`[GDE-BMK-020]`) and the
> boundary §4 explains (`[GDE-LES-080]`)

---

## 1. The data model — reused, not rebuilt

**`[SPEC-PREF-010]`** `listener_preferences` (`subject_kind` ∈
{`recording`,`artist`}, `subject_id` = mbid, `rotation`/`recovery`/
`restraint` REAL, `updated_at`) already existed, unchanged from the
MuLibPlay migration. No new table. `NULL` in any column means "not tuned,
use the Program Director's own default" (`Tuning::recording_defaults()`/
`artist_defaults()`, `player/src/director/frequency.rs`) — never a
fabricated zero, and never silently filled in server-side, so a caller can
always tell "unset" from "explicitly set to the default value."

**`[SPEC-PREF-015]`** A row absent entirely (subject never edited) reads
identically to a row present with every column `NULL` — `get_preference`
returns three `None`s either way, so nothing calling it has to special-case
"no row yet" against "row exists, nothing tuned."

## 2. The read/write contract

**`[SPEC-PREF-020]` `GET /preference/:kind/:id`** → `{rotation, recovery,
restraint, defaults: {rotation, recovery, restraint}}`, `kind` ∈
{`recording`,`artist`}, `400` otherwise. Every field independently
nullable; `defaults` always concrete, from `frequency.rs`'s own constants —
not re-typed as a second copy of them.

**`[SPEC-PREF-025]` `POST /preference/:kind/:id?rotation=&recovery=&restraint=`
— a three-way query, not a plain value set.** A field **absent** from the
query string is left exactly as stored; **empty** (`?rotation=`) resets it
to "use the default" (`PlayerStore::reset_preference`); a **number** sets
it (`PlayerStore::set_preference`). This is why the query is read as a
plain string map (`axum::extract::Query<HashMap<String,String>>`) rather
than a typed extractor, which would collapse "absent" and "empty" into the
same value and make a client unable to say "leave rotation alone, but
clear restraint" in one request.

**`[SPEC-PREF-030]` `set_preference`'s own `COALESCE`-based upsert cannot
clear a field — that is deliberate.** A caller passing `None` for one field
means "this save did not touch it," so a slider drag on rotation alone
must never blank a restraint value it never looked at. `reset_preference`
is the one operation that actually writes `NULL`, and it is always a
separate, explicit call.

**`[SPEC-PREF-035]` A successful write requests a Director reload the same
way `POST /library/reload` does** (`control.rs`'s `reload_library` —
`ui.controls.lock().reload_requested = true`). One request both saves the
value and lets the running engine pick it up on its own next refill; the
client never has to make a second call.

## 3. Where it's reachable

**`[SPEC-PREF-040]`** A displayed title or artist becomes a clickable link
wherever a real mbid is already known — unidentified audio or an
uncredited artist stays plain text, the same case that already leaves the
name itself unbadged (`[REQ-VIS-120]`). In scope: the Vaino skin's
now-playing row, queue, and history table; the MuLibPlay skin's
now-playing row and queue. Out of scope, named rather than silently
dropped: the WinAmp skin, whose marquee concatenates artist and title into
one scrolling string with no separate DOM node to link — a materially
bigger restructuring of a skin this feature was never asked to touch.

**`[SPEC-PREF-045]` One shared panel, built once in `core.js`, not a
bespoke editor per skin.** No modal/dialog convention exists anywhere in
this codebase — every interactive surface is either a `hidden`-toggled
sibling `<section>` (the Vaino skin's own `panel-main`/`panel-settings`/
`panel-history` switch) or an inline card list (`review.html`). MuLibPlay's
skin has neither kind of infrastructure to extend. Rather than invent two
different mechanisms, `Vaino.editPreference(kind, id, label)` builds a
single floating panel, lazily, appended to `document.body`, restyleable by
any skin's own CSS via `.pref-panel`/`.pref-box`/`.pref-link` but requiring
no per-skin markup.

**`[SPEC-PREF-050]` Rotation and recovery are shown as a human duration,
restraint as a human multiplier — not the raw log-scale float.** Client-side
readouts reuse `frequency.rs`'s own formulas (`10^v` hours; `10^-v`) for
display only; the server remains the sole place a value actually takes
effect, so a display bug here can misrepresent a number but never change
what gets stored.

## 4. What this does not do

**`[SPEC-PREF-060]` This does not relitigate `[GDE-LES-080]`.** GUIDE001
records this project's own retrospective judgment that direct
slider-tuning was the wrong *primary* way for a listener to express taste
going forward — "naming six songs beats tuning eleven sliders" — favoring
the exemplar/seed-track model instead. That lesson is about Like/Dislike/
Taste, a genuinely separate system this document does not touch. What this
restores is narrower and already-designed-for: `listener_preferences` was
real, used MuLibPlay functionality that Vaino's own schema already carries
forward, with nothing anywhere to reach it. Building an editor for data
already being carried is not the same claim as choosing sliders over seeds
as the primary preference mechanism.

**`[SPEC-PREF-065]`** The two global `listener_settings` master time-scale
multipliers (`[SPEC-DIR-118]`) are a different, whole-library control —
not per-subject, not touched here.

---

## 5. Open

**`[SPEC-PREF-070]`** The WinAmp skin (§3) — deferred, not designed against;
would need its marquee restructured to carry artist and title as separate
nodes before it could opt in at all.

---

**Traceability:** `[SPEC-PREF-010..070]` · derives `[REQ-VIS-285]`,
`[REQ-VIS-290]` · reuses `listener_preferences` (`[SPEC008]`) and the
`Tuning` defaults/formulas (`[SPEC009]` `[SPEC-DIR-110]`, `[SPEC-DIR-115]`)
· restores, does not replace, `[GDE-BMK-020]`'s migrated MuLibPlay data;
does not touch `[GDE-LES-080]`'s Like/Dislike/Taste scope
