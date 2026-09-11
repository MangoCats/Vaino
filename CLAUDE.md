# Working in this repository

Conventions that are **not discoverable from the code**, and that cost real
time when they are unknown. Everything here is written because it actually
went wrong, not as general advice.

---

## 1. `cargo fmt` is off limits

**This repository is hand-formatted. Do not run `cargo fmt`.** Use
`cargo clippy` when you want feedback on the code — it reports real problems
and changes nothing on its own.

This is enforced mechanically by [`player/rustfmt.toml`](player/rustfmt.toml)
(`disable_all_formatting = true`), so a `cargo fmt` is now a no-op rather than
a disaster. The number behind that: measured 2026-09-11, `cargo fmt --check`
wanted to rewrite **959 diff lines** across the crate.

The reason is not taste. The code carries unusually long explanatory
comments — measurements, dated findings, why a constant has the value it has —
and their line breaks are part of how they read. rustfmt reflows them, and the
resulting mechanical diff buries whatever real change it is mixed with.

**If you are one of several agents in this repo, assume the others have not
read this.** The rule existed only in one contributor's memory until
2026-09-11, which is exactly how it got broken: an agent working on Sampo ran
`cargo fmt`, reformatted `player/src/web/sampo.rs`, and had no way to know a
convention existed. A convention only one participant can see is not one.

## 2. Unset `CC` before building the player

`libsqlite3-sys` fails to link if a global `CC` is set. Every build script
here already does this — see `env -u CC cargo build` in
[`build/deploy-local.sh`](build/deploy-local.sh) — but a build typed by hand
will not, and the error it produces does not point at the cause.

## 3. Documentation has enforced governance

Run [`tools/check_docs.py`](tools/check_docs.py) before committing any
documentation change; CI runs it with `--strict` on every push. It fails on
dangling tags, identifier collisions, unresolvable links, and cited file paths
that do not exist — so a rename and its documentation citations **must land in
one commit**.

- Every requirement/spec/finding carries a bracketed tag `[GOV-DOC-010]`.
  A tag at the start of a line reads as its *definition*; cite one mid-line.
- **300 lines is a hard limit** per document, 100–250 the target. Above 300,
  split rather than trim.
- Machine-specific material lives in that machine's folder (`VainoPi/`,
  `BosePi/`, `SmartPC/`); generic tooling does not `[GDE-DEP-040]`.
- [GOV002](docs/GOV002-sources-of-truth.md) is the house discipline: when two
  sources answer one question, rank them **by measurement**, and make a
  fallback visible in the output rather than silently equivalent.

## 4. Deploying to an appliance is not a file copy

`bose` has an **overlay root**: an ordinary write to `/` lands in a tmpfs upper
layer, survives a service restart, passes every check — and is gone at the next
reboot. Five days of player deploys went that way
`[IMPL-BOS-185]`. `vainopi` has a plain writable root and does not.

- Use [`build/deploy-appliance.sh`](build/deploy-appliance.sh) for the player
  and [`build/install-config.sh`](build/install-config.sh) for any file; both
  detect the overlay and write **both** layers.
- For a package, or anything needing a root environment, use
  `sudo overlayroot-chroot` — see
  [BOSE010](BosePi/BOSE010-changing-a-locked-card.md).
- **Verify the durable copy, never the running one** `[GDE-DEP-070]`. Asking
  the live process proves only what is running now.

## 5. Say what you assume about a target before acting on it

`[GDE-DEP-060]`. A script that silently assumes a machine's shape produces a
log indistinguishable from one that assumed correctly. State it — *"pi@bose
has an overlay root; will persist through /media/root-ro"* — so a wrong guess
becomes a visible line rather than a silent success.

The same applies to checks: a guard that cannot run must say so loudly, not
report a plausible reason it was skipped. One written here did exactly that
and went unnoticed through a full fleet deploy.

## 6. Several agents may be working here at once

**Never `git add -A` or `git commit -a`. Stage the paths you actually
changed.** Another agent's in-progress work lives in the same working tree,
and a blanket stage sweeps it into your commit.

This is not hypothetical. On 2026-09-11 commit `294ab3e`, whose message is
entirely about deploy scripts, also carries 98 changed lines of
`player/src/web/sampo.rs` and 48 of `tools/console.py` — a genuine Sampo fix
for dead-but-bound console detection, written by a different agent, committed
and pushed under an unrelated message. Nothing was lost, but only because that
edit happened to be finished; a half-written one would have been published just
as readily, and the author had no way to know.

For the same reason, **check before reverting a file you did not change.**
`git status` and `git log --oneline -5` cost nothing. A `git checkout --` on a
file someone else is editing discards work that was never yours to discard.

Do not rewrite shared history to tidy any of this up — a misleading commit
message is a much smaller problem than a rebased branch under someone else's
feet.
