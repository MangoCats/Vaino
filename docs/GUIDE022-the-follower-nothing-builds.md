# GUIDE022: The Follower Nothing Builds

**Development Guidance — written 2026-09-18, split out of
[GUIDE021](GUIDE021-echo-review.md) when acting on it grew past that document's
limit**

Three things a read of the source could not see, found while fixing what
[GUIDE021](GUIDE021-echo-review.md) `[GDE-ECHO-371]` did see. Two are small
corrections to that review. The third is larger than anything in it, and is
the same fault one level up: a mechanism that declines silently.

> **Related:** [GUIDE021](GUIDE021-echo-review.md) `[GDE-ECHO-371]` — the review this came out of · [GUIDE017](GUIDE017-echo-correction.md) `[GDE-ECHO-340]` — the loop being reviewed · `player/Cargo.toml` — where the gate is · `build/deploy-appliance.sh` — what does not pass it

---

## 1. The gate

**`[GDE-ECHO-379]` `player/src/echo_client.rs` is behind a cargo feature that
nothing in the tree turns on.** So the follower is absent from every binary
any script here produces, while the control that starts it is present in every
interface.

`echo-client` is not in `default`. Searching the tree for anything that enables
it finds nothing: not `cargo test`, not `build/verify-targets.sh`, not
`build/deploy-local.sh`, not `build/deploy-appliance.sh` — whose `FEATURES` is
`${VAINO_FEATURES:-}`, empty unless a caller sets it — and no document
anywhere names `--features echo-client`. The live two-node measurements
recorded in `[GDE-ECHO-341]`, `[GDE-ECHO-342]` and `[GDE-ECHO-347]` were
therefore taken against a binary built by hand, by a route nothing in the
repository describes.

What that costs is not that following is missing. It is **how** it is missing.
`vaino.rs` spawns the follower task inside `#[cfg(feature = "echo-client")]`,
and refuses the `--follow` flag loudly without it `[GOV-SRC-040]` — that half
is right. But the setting is the control `[SPEC-ECHO-010]`: the settings panel
offers *Follow this node*, the browser posts a host, `SetEchoFollow` stores it,
`remember_settings` persists it, and the panel then shows a node configured to
follow. Nothing reads it. `follow_status` stays empty, which the interface
renders as a node that has simply not connected yet. A listener has no way to
tell that from a network fault.

That is `[GDE-ECHO-378]`'s shape exactly, and the deploy script's own comment
about `sampo-support` already states the principle it breaks: *"a UI that looks
like it works and does not, which is worse than a 404."*

**`[GDE-ECHO-384]` It also means the findings were never compiled.** Every
fault GUIDE021 records in `echo_client.rs` — the early returns
`[GDE-ECHO-374]`, the mid-join budget `[GDE-ECHO-375]`, the flow comparison
`[GDE-ECHO-376]` — sat in a file that `cargo test` does not build. Not
untested: **uncompiled**. A rename would not have caught them, clippy would not
have caught them, and no test could have caught them, which is a fair part of
why they survived to be found by reading.

**`echo-client` is now in `default`, decided 2026-09-18 by what the fleet
turned out to be.** The deployed binaries settled it rather than the argument
did. `pi@bose` carries five `echo-follow:` strings and none of the
`#[cfg(not(...))]` refusal, so it *was* built with the feature — by hand,
through `VAINO_FEATURES`, by a route nothing here describes. `pi@vainopi` has
neither. Two nodes of one fleet, differing in whether a control works, because
the correct build depended on an environment variable a person has to
remember.

That makes the third way out — hiding the control when the feature is off —
the wrong one: it would make the divergence a feature. And it makes the second
— setting `VAINO_FEATURES` at deploy time — the mechanism that caused this,
chosen again. It also means a deploy that did *not* set it would have silently
removed the follower from `bose`, which is the fault running in reverse.

So: `default = ["echo-client"]`. The dependencies were always free —
`tokio-tungstenite` and `futures-util` are in the graph via axum's own `ws`
feature — and the cost is about 400 kB of appliance binary, against a control
that is offered on every node and worked on one.

---

## 2. Two corrections to the review

**`[GDE-ECHO-381]` `echoprobe` does not exercise `trim_for`, so nothing
does.** `[GDE-ECHO-377]` names it as the one remaining caller. It calls
`trim_for(st, None, now)`, and that function's second act is `let local =
local?` — it returns on a `None` air position before reaching any arithmetic.
The probe's own "would `DropFrame`" line is unreachable, and its comment
beside it ("no local air position here, so this is not expected") is the
closest thing in the tree to saying so.

The verdict is unchanged and slightly stronger: `residual_ns` and `trim_for`
were not two formulas with one caller between them, they were two formulas
with none. That is why deleting the first cost nothing and why the second now
carries a note about the clock translation an adopter must add
`[GDE-ECHO-366]`.

**`[GDE-ECHO-382]` The mid-join threshold is 600 ms, not "about 500".**
`[GDE-ECHO-375]` computes when a join lands `TooLate` from `skip_lead_ms +
echo_prep_ms` against the 1 s margin, and omits `ECHO_START_LATE_LIMIT`'s own
100 ms of grace. Immaterial to the finding — `ECHO_PREP_MAX_MS` is 2000 either
way, and the first join on a cold node guesses 400 — but the arithmetic is
worth having right, because it is the figure a fix has to clear.

---

## 3. What to take from it

**`[GDE-ECHO-383]` A test that cannot compile the file is not a weaker test
than one that does not assert; it is a different kind of absence, and only one
of them shows up in a count.** `[GDE-ECHO-378]` says the boundary between
deciding and acting was the only place in this subsystem with no assertion
across it. That was true of the code that was built. The subsystem also had a
whole file with no *compilation* across it, and 506 passing tests said nothing
about either.

The cheap general guard is the same one `[GDE-ECHO-378]` asks for, applied to
the build rather than to an actuator: **a feature that gates behaviour a
running interface still offers must be named somewhere a build reads**, or the
gate is not a gate, it is a silence.

**`[GDE-ECHO-385]` A third instance turned up while shipping the first two,
and it is the same sentence again.** `web::Snapshot` carried `echo` and not
`echo_node`, while `skin.js` reads `s.echo_node` — so `renderEchoNode` took
its `if (!n) return;` twice a second and the delay, calibration and follow
panel showed nothing on every node that has ever run. Neither guard could see
it: the Rust test lists snapshot fields by hand from the server's side, and
`build/verify-skins.js` renders against fixture snapshots written by hand too,
so both agreed with each other and neither with the skin. The new test derives
the list from the skin — every `s.<field>` in `skin.js` must be a field the
snapshot serialises — which found `echo_node` and one more, `sink`, read by
the Bluetooth helper and carried by no snapshot ever.

Three faults, three mechanisms, one shape: **an actuator, a build and a wire
each declining silently one call below something whose tests pass.** The guard
that catches all three is not a better test of either side. It is an assertion
that reads *the other side* — what admission will really spend, what the build
really compiles, what the skin really asks for.
