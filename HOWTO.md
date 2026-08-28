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

## 4. Run Vaino

```
player/target/release/vaino vaino.db --port 5720
```

Open `http://127.0.0.1:5720/` for the player UI. `--port` defaults to
`5720` if omitted; `--device NAME` picks an output device by a
case-insensitive substring match if the default one isn't what you want.

---

## 5. Run Sampo

Sampo's console is a plain Python script, no build step:

```
python tools/console.py vaino.db --root "/path/to/your/Music"
```

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

- `README.md` — what Vaino is and why.
- `docs/GUIDE001-lineage-and-lessons.md` — start here for the project's
  own history and design lessons.
- `docs/spec/SPEC013-sampo-console.md` — the console's own design.
- `docs/IMPL003-sampo-console-build.md`, `IMPL006`, `IMPL007` — what's
  actually built, stage by stage, with measured results.
- `VainoPi/` — building and deploying the Raspberry Pi appliance image.
- `build/README.md` — cross-compilation and the Windows `CC` trap in full.
