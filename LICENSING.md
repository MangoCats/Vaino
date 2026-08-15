# Licensing

Two works, two licences, one repository.

| Path | Work | Licence |
| :--- | :--- | :--- |
| `player/`, `docs/`, `build/`, `sql/`, root files | **Vaino** — the player | **MIT** |
| `tools/` | **Sampo** — the library builder | **AGPL-3.0-or-later** |

Full texts: [`LICENSE`](LICENSE) (MIT) and [`tools/LICENSE`](tools/LICENSE) (AGPL-3.0).

## Why they differ

Sampo's job is acoustic analysis, and the library that does it well — Essentia
— is AGPL. A work that builds on it inherits those terms. Vaino has to stay
permissive: it is the part people run on their own hardware, port to their own
platforms, and embed in appliances.

**The direction is deliberate and it only works one way.** MIT code may be
incorporated into an AGPL work; AGPL code may not be incorporated into an MIT
one. So the shared parts — the schema, the specifications, the player — are
MIT, and Sampo takes on AGPL without that obligation flowing back
(`[GDE-ARC-018]`, `[SPEC-SA-010]`).

## What keeps them separate

They are separate programs, not a linked whole:

- different languages — Rust and Python — compiled and run independently;
- no shared process, no RPC, no linked code in either direction;
- **the only channel between them is a shared SQLite file** (`[SPEC-SA-015]`).

Sampo runs on demand on a desktop and writes `vaino.db`. Vaino reads it. A
Vaino installation needs no part of Sampo present to run, and nothing under
AGPL is incorporated into the player.

## If you are contributing

Patches to `tools/` are AGPL-3.0-or-later. Patches to everything else are MIT.
Each Python file in `tools/` carries an `SPDX-License-Identifier` line saying
so, so the terms travel with the file rather than depending on where it sits.

## Status

`[SPEC-SA-010]` describes Sampo as a separate project with its own repository.
Relicensing in place is a deliberate interim: the boundary that matters
technically — separate programs, shared file only — already holds, and
splitting the repository can happen later without changing anyone's terms.

Two things worth knowing about `tools/` as it stands:

- it mixes Sampo's pipeline with research scripts and dev utilities, so the
  line to cut along if the repository is ever split is not yet drawn;
- `tools/check_docs.py` is a documentation checker with no connection to
  Sampo, and is AGPL only because of where it lives.
