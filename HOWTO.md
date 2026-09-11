# HOWTO: build and run Vaino and Sampo locally

Quick, practical steps for a desktop dev machine — not the appliance build
(see `VainoPi/` for that) and not the full architecture (see `README.md`
and `docs/` for that). Two things get built and run:

- **Vaino** (`player/`) — the Rust player. Plays music, serves the web UI.
- **Sampo** (`tools/`) — the Python library builder and its browser console.
  Reads and writes the same SQLite file Vaino plays from; the two never
  talk to each other directly except through that file, plus one narrow
  handoff described at the end.

Every command below is verified against this repository as it stands.

---

## 1. Prerequisites

- **Rust** (stable toolchain) for Vaino.
- **Python 3** for Sampo. Nothing needs `pip install` for the steps below —
  `tools/requirements.txt` is only for feature-extraction work (Stage-B
  models, AcousticBrainz dump processing), not for building a library or
  browsing the console.
- **On Windows, unset `CC` before building.** If it's set globally to a
  MinGW compiler, the bundled SQLite is compiled with MinGW while `rustc`
  links with MSVC, and the build fails on an unresolved `___chkstk_ms`:

  ```
  env -u CC cargo build --release
  ```

  (`build/README.md` has the full explanation and the cross-compile story
  for the Pi, if you need that instead.)

---

## 2. Build Vaino

```
cd player
env -u CC cargo build --release
```

This gives you `player/target/release/vaino` (`vaino.exe` on Windows) — the
plain appliance-equivalent build.

**If you also want the review page, the waveform editor, and MusicBrainz
search reachable from Sampo**, build with the feature that carries them —
off by default because an appliance that never runs Sampo has no reason to
carry the extra ~200 KB or the `reqwest` dependency it pulls in:

```
env -u CC cargo build --release --features sampo-support
```

Either binary plays music and serves the ordinary web UI identically; the
feature only adds routes Sampo's console links to.

---

## 3. Get a library

Vaino and Sampo both need a `vaino.db` — a SQLite file matching
`sql/schema.sql`. If you don't already have one:

```
python3 -c "import sqlite3; sqlite3.connect('vaino.db').executescript(open('sql/schema.sql', encoding='utf-8').read())"
```

That gives you an empty, valid library — enough to run both tools and look
around, but nothing plays until it has music in it. To induct a real
folder, run Sampo's pipeline against it in order:

```
python tools/ingest_folder.py vaino.db "/path/to/some/album" --commit
python tools/extract_library.py vaino.db
python tools/fingerprint_ids.py vaino.db
python tools/fingerprint_ids.py vaino.db --merge
```

- Drop `--commit` from the first command to see what it *would* do first —
  every one of these tools rehearses by default except where `--commit` is
  given.
- `fingerprint_ids.py` needs network access (AcoustID) and can take a
  while over a large folder; skip it for a quick local test and Vaino will
  still play the music, just without identified recordings for it yet.
- All of this is also reachable from Sampo's own browser console (§5,
  the "jobs" and "export" pages) once it's running, rather than the CLI.

---

## 3b. One database or two

A library can be one file or two, and everything here works either way.

**One file** is the simple case and the default: `vaino.db` holds the
catalogue (files, passages, recordings, flavor) and the listener's own
state (play history, preferences, programmes) together. Nothing below needs
a second path.

**Two files** separate them — `library.db` for the catalogue, `listener.db`
for everything the listener created. The appliance needs this because it
keeps the catalogue on a read-only partition `[PI023]`; the desktop was
split on 2026-09-11 so that the split shape is the one being exercised
daily, rather than a shape only the Pi ever runs and only the Pi ever finds
bugs in.

```
python tools/split_database.py vaino.db \
    --library-out library.db --listener-out listener.db          # rehearse
python tools/split_database.py vaino.db \
    --library-out library.db --listener-out listener.db --commit
```

It never modifies the source, refuses to overwrite an existing output, and
verifies row counts table-for-table, `integrity_check` and every index
before reporting success. Keep the original: it is the rollback.

Afterwards, **Sampo's tools take either path and find the other one**, so
long as the two sit in the same directory under those names. Where they do
not — the appliance keeps them on separate mounts — name the second one:

```
python tools/load_occasions.py listener.db --library /srv/library/library.db
VAINO_LIBRARY=/srv/library/library.db python tools/export_flags.py listener.db -o flags.json
```

**Sampo's console keeps a sidecar beside whichever database it was given**
— `<name>.console.db`, holding job history and the remote-peer
configuration. Splitting changes the name, so the sidecar has to come with
it, or the console starts with an empty job list and no configured peer and
nothing says why:

```
cp vaino.console.db library.console.db
```

`python tools/audit_split_readiness.py` lists every script and whether it
goes through the shared opener; `python tools/test_split_parity.py --pair
DIR --whole vaino.db` runs the read-only ones against both shapes and
compares the answers.

---

## 4. Run Vaino

```
player/target/release/vaino vaino.db --port 5720                      # one file
player/target/release/vaino listener.db --library library.db --port 5720   # two
```

Open `http://127.0.0.1:5720/` for the player UI. `--port` defaults to
`5720` if omitted; `--device NAME` picks an output device by a
case-insensitive substring match if the default one isn't what you want.
`--library` is the catalogue half where the database has been split; the
player attaches it read-only and writes only the listener half.

---

## 5. Run Sampo

Sampo's console is a plain Python script, no build step:

```
python tools/console.py vaino.db --root "/path/to/your/Music"       # one file
python tools/console.py library.db --root "/path/to/your/Music"    # two
```

Given either half of a split library it finds the other beside it, so there
is no second path to pass here.

Open `http://127.0.0.1:5730/`. `--port` defaults to `5730`. `--root` is
repeatable and points at the audio folder(s) the "folder" view compares
against what the database claims — omit it and everything else still
works, just with an empty folder view.

The console opens the database `mode=ro`: it cannot write to your library
no matter what happens in the browser. Everything that writes runs as a
separate job, subprocessing the same CLI tools shown in §3.

---

## 6. How they meet: the handoff

A few pages Sampo links to — reviewing a questionable recording id,
editing a passage's waveform boundaries — are actually served **by
Vaino**, not by the console. The first time you open one of those links,
Sampo checks whether a Vaino is already listening on `127.0.0.1:5720` and,
if not, launches one itself — built with `--features sampo-support` (§2),
on Sampo's own database path — and waits for it to answer before handing
you off. If no such binary exists yet, the page says so by name instead of
showing a dead link.

This means: build the `sampo-support` binary (§2) if you want those pages
to work, and keep it at `player/target/release/vaino[.exe]` (or on `PATH`)
— that's where Sampo looks for it.

---

## Where to go next

- Once Vaino is running (§4), it serves its own in-app user's guide at
  `/guide`, also reachable from a **Help** link in every skin — a
  multi-tiered, listener-facing document (quick start through advanced
  features and an algorithm appendix), distinct from this file and from
  `README.md`, both of which stay developer-facing.
- `README.md` — what Vaino is and why.
- `docs/GUIDE001-lineage-and-lessons.md` — start here for the project's
  own history and design lessons.
- `docs/spec/SPEC013-sampo-console.md` — the console's own design.
- `docs/IMPL003-sampo-console-build.md`, `IMPL006`, `IMPL007` — what's
  actually built, stage by stage, with measured results.
- `VainoPi/` — building and deploying the Raspberry Pi appliance image.
- `build/README.md` — cross-compilation and the Windows `CC` trap in full.
