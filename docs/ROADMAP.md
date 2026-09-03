# ROADMAP: What's Open, Across the Project

**Orientation — Tier 1 · forward-looking only, added 2026-09-03 per `[GOV-DOC-050]`**

A single scan point for "what's not built yet." Most open items live in the
document whose subject they belong to — this page mostly **links** rather than
duplicates, so a change to one of those sections doesn't also require an edit
here. The prose sections below hold only the material that doesn't belong to
a single document: cross-cutting plans, or an investigation that was never
folded into one spec.

> **Related:** [architecture.md](architecture.md) for what's built ·
> [GOV001](GOV001-document-hygiene.md) `[GOV-DOC-050]` for the convention this
> page exists to satisfy · [GUIDE001](GUIDE001-lineage-and-lessons.md) for the
> historical counterpart to this page

---

## 1. Index — every document's own "Open" section

| Area | Document | Section |
| :--- | :--- | :--- |
| System-wide gaps | [architecture.md](architecture.md) | [§10 Known gaps](architecture.md#10-known-gaps) |
| Functional requirements | [REQ002](spec/REQ002-functional-requirements.md) | [§8 Coverage Gaps](spec/REQ002-functional-requirements.md#8-coverage-gaps) |
| Flavor distance | [SPEC005](spec/SPEC005-flavor-distance.md) | [§5a Future Direction — entry/exit flavor](spec/SPEC005-flavor-distance.md#5a-future-direction--entry-and-exit-flavor) |
| Sampo pipeline | [SPEC007](spec/SPEC007-sampo-architecture.md) | [§6 Segmentation & Amplitude — PROVISIONAL](spec/SPEC007-sampo-architecture.md#6-segmentation--amplitude-s2-s6--provisional) |
| Database schema | [SPEC008](spec/SPEC008-database-schema.md) | [§8 Open](spec/SPEC008-database-schema.md#8-open) |
| Program Director | [SPEC009](spec/SPEC009-program-director.md) | [§9 Open](spec/SPEC009-program-director.md#9-open) |
| Audio path supervisor | [SPEC011](spec/SPEC011-audio-path-supervisor.md) | [§5 Risks and open questions](spec/SPEC011-audio-path-supervisor.md#5-risks-and-open-questions) |
| Library relink | [SPEC012](spec/SPEC012-library-relink.md) | [§7 Decided, and deferred](spec/SPEC012-library-relink.md#7-decided-and-deferred) |
| Sampo console | [SPEC013](spec/SPEC013-sampo-console.md) | [§6 Open](spec/SPEC013-sampo-console.md#6-open) |
| Payload schema | [SPEC014](spec/SPEC014-payload-schema.md) | [§6 Open](spec/SPEC014-payload-schema.md#6-open) |
| MPD Director | [SPEC015](spec/SPEC015-mpd-director.md) | [§8 Open](spec/SPEC015-mpd-director.md#8-open) |
| Waveform boundary editor | [SPEC021](spec/SPEC021-waveform-boundary-editor.md) | [§6 Not yet measured](spec/SPEC021-waveform-boundary-editor.md#6-not-yet-measured) |
| Phone ports | [GUIDE004](GUIDE004-phone-port-strategy.md) | [§7 Open](GUIDE004-phone-port-strategy.md#7-open) |
| Hosted flavor service | [GUIDE005](GUIDE005-flavor-service.md) | [§5 Open](GUIDE005-flavor-service.md#5-open) |
| Director as a guest | [GUIDE006](GUIDE006-director-as-a-guest.md) | [§5 Open](GUIDE006-director-as-a-guest.md#5-open) |
| External backends | [GUIDE007](GUIDE007-external-backends-investigation.md) | [§7 Open](GUIDE007-external-backends-investigation.md#7-open) |
| Appliance setup | [VainoPi/IMPL001](../VainoPi/IMPL001-appliance-setup.md) | [§9 Open](../VainoPi/IMPL001-appliance-setup.md#9-open) |
| Image & partitions | [VainoPi/PI001](../VainoPi/PI001-image-and-partitions.md) | [§7 What is not yet decided](../VainoPi/PI001-image-and-partitions.md#7-what-is-not-yet-decided) |
| Appliance characterisation | [VainoPi/PI006](../VainoPi/PI006-appliance-characterisation.md) | [§9 What was not measured](../VainoPi/PI006-appliance-characterisation.md#9-what-was-not-measured) |
| MPD on the appliance | [VainoPi/PI007](../VainoPi/PI007-mpd-on-the-appliance.md) | [§4 What was not measured](../VainoPi/PI007-mpd-on-the-appliance.md#4-what-was-not-measured) |

## 2. Sendspin — a whole directory of "watch, don't build yet"

[`sendspin/`](../sendspin/) holds six investigation documents (SPIN001–006)
into whether/how Vaino should interoperate with the Sendspin multi-room
ecosystem, Music Assistant, and OpenSubsonic. Every one of them is already
structured as current analysis plus its own small "Open"/"Recommendation"
section — nothing here is built, and the standing recommendation across all
six is to watch rather than commit engineering time. Read
[SPIN001](../sendspin/SPIN001-protocol-and-integration-analysis.md) first;
each of the other five narrows to one sub-question it raised.

## 3. Rearchitecture — what's still ahead

*(From [GUIDE002](GUIDE002-rearchitecture-plan.md)'s phased plan and open
questions — see [GUIDE001 §8](GUIDE001-lineage-and-lessons.md#8-rearchitecture-phases--retrospective)
for the phases that have already shipped.)*

### P4 — Ingest & DAO Segmentation

**Reproduce** McRhythm's cascade `[GDE-MCR-010]` — not merely port it. The inherited [MCR-SPEC033](inherited/mcrhythm/MCR-SPEC033-album_matching.md) describes *McRhythm's implementation*, not Vaino's contract, so reproduction has three deliverables in order:

1. **Requirements** — what Vaino demands of segmentation, independent of how McRhythm did it. Source material: MCR-SPEC033, MCR-IMPL005.
2. **Specification** — Vaino's own cascade spec: grid search → DP assembly → RMS quiet-spot → extra merging, 7-strategy MusicBrainz edition search, windowed-dB-profile optimization, Stage-6 RMS boundary refinement.
3. **Implementation + review UI** — waveform with draggable boundaries, lead-in/lead-out markers and gain `[SPEC-SA-080]`; fade-in/fade-out markers and curves `[SPEC-SC-046]` were added to the same editor after the fact, once real playback turned out to need them.

`start_ms`/`end_ms` (the `radio`-kind's own trim) and `lead_in_ms`/`lead_out_ms` are **computed**, from the inherited amplitude analysis [MCR-SPEC025](inherited/mcrhythm/MCR-SPEC025-amplitude_analysis.md) `[SPEC-SA-075]` — this supplies the Radio side of the Album/Radio duality `[GDE-BMK-030]` that MuLibPlay only ever produced by hand. Automatic placement is always reviewable and overridable; manual edits outrank computed values permanently.

Every decision persisted as an inspectable **ingest decision record** — which stage matched, at what confidence, which editions were considered and rejected `[GDE-CHT-030]`. This turns "undocumented ritual" into "reviewable process".

> **Reports:** measured against McRhythm's 93% album match / 96% mean boundary accuracy `[GDE-MCR-010]` on the 189 known DAO files. **These must be independently re-verified** — at present they are simultaneously the target and the only evidence, which is not a test.

*(Status note: `tools/segment_dao.py` implements a threshold-sweep silence detector, measured at 16/20 (80%) exact track-count match and 94% of boundary starts within 2s on the hardest known files, and now wired to `--commit`, but this is not the McRhythm cascade above — no grid search, no DP assembly, no MusicBrainz edition search. [SPEC007](spec/SPEC007-sampo-architecture.md) §6 stays PROVISIONAL pending this work.)*

### Open questions

1. **Which user-defined characteristics to define first?** `[GDE-ARC-030]` supports them generally; MuLibPlay's six years of use suggest christmas / winter / summer / kids are the proven ones `[GDE-PD-020]`.
2. **Wall Art / Kiosk display mode — dropped, or merely unrevisited?** The pre-rearchitecture plan (`docs/user-interface.md`, now deleted) specified a fullscreen wall-tablet mode: large album art, clock, upcoming-track cards, OLED/LCD burn-in protection. Grep for `wall.art|kiosk|burn.in` across `player/` and `tools/` returns nothing — it was never built, and the current skin model (`vaino`/`mulibplay`/`winamp`, all document-shaped, `[REQ-VIS-160]`) has no kiosk-style skin among them. Nothing in `REQ002` accepts or rejects it. If wanted, it is a fourth skin under the existing contract; if not, this line is where that should be said.
3. **The Phase 7 feature list — dropped, or merely unrevisited?** The old roadmap's final phase (`docs/roadmap.md`, now deleted) named station-ID/jingle injection between tracks, news/weather TTS announcements, and MQTT/smart-home hooks. None appear in `REQ002` or any current `SPEC`, and none exist in code. This is *not* the same question as scrobbling — `[SPEC-MPD-100]` already, deliberately, declines that one for the MPD guest path specifically, reasoning that guest clients already scrobble. The other three were simply never revisited after the rearchitecture and carry no decision either way.
4. **Library-browse pagination — was the REQ001-era page-size selector dropped on purpose?** The deleted `REQ001`'s `[REQ-UI-020K]` specified a page-size dropdown (`10`/`25`/`50`/`100`/`250`) with dynamic Prev/Next controls. The built `/browse` route (`player/src/web/browse.rs`, split out of `web.rs` 2026-09-02) instead caps every response at a flat 2,000 rows with no selector `[REQ-VIS-180]`. That may be the right call for a LAN player with a "Built for a phone" design brief — a flat cap is simpler and 2,000 rows is generous — but it was never stated as a deliberate simplification, only as an absence.

## 4. The appliance's still-open speaker questions

**Does a reopened Bluetooth output stream actually hold indefinitely, or does
the 700 ms settle merely push the failure further out?** Investigated
2026-08-16 in [PI003 §4a](../VainoPi/PI003-choosing-a-speaker.md#4a-the-reopened-stream-and-feeding-silence-while-paused);
the incident and what was tried is recorded in
[PI008 §2](../VainoPi/PI008-appliance-bringup-history.md#2-a-reopened-stream-dies-a-fresh-one-does-not).

A stream rebuilt against an already-running player (`recover()`) was found to
lose the speaker about twenty-two seconds after reopening, every time, while a
stream opened fresh at startup held indefinitely. Giving PipeWire 700 ms to
finish tearing down the old stream before opening the new one closed the gap
in testing — two minutes, connected on every sample, from the worst case
(player already running, no speaker, then connect and select).

That two-minute result is not enough to trust on its own: an earlier
two-minute test had already been called "verified" once, and the drop
actually arrived twenty seconds after that window closed, at about two and a
half minutes. No test run so far has gone longer than the failure interval it
was trying to rule out. **Still open:** whether the 700 ms settle actually
fixes the fragility, or only delays it past whatever window each test
happened to run, is unconfirmed. Closing this needs a run of at least ten
minutes with the underrun counter watched throughout — the signal that
caught the fault both previous times it mattered — before the reopened-
stream path can be called reliable rather than merely improved.

---

**Traceability:** exists to satisfy `[GOV-DOC-050]` · nothing here is a
requirement or a spec in its own right — every open item traces back to the
`REQ`/`SPEC`/`GDE` tag in the document it's linked from.
