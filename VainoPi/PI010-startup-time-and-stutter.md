# PI010: Startup Time, and Hunting a Stutter

**Appliance Record — where 19 seconds went, and four causes that were not it**

Split from [PI004](PI004-speaker-operation.md) on 2026-09-08. Continues
[PI009](PI009-the-silence-of-2026-09-08.md). Memory, swap, CPU starvation and
SD contention were each proposed for the periodic stuttering and each measured
away here; the answer is in [PI011](PI011-two-speakers-and-placement.md).

The instruments built to settle it — and the one that had to be rewritten
because it was a suspect in its own measurements — are documented in §3, with
the commands to switch them on again.

> **Related:** [PI004](PI004-speaker-operation.md) §0 for current understanding ·
> [SPEC011](../docs/spec/SPEC011-audio-path-supervisor.md) for the audio path
> supervisor these findings kept meeting

---

## 1. Startup stutter, and the observer that caused some of it

**`[PI3-FOUND-170]` Diagnosing this over ssh perturbs the thing being
measured.** The Pi Zero 2W puts Wi-Fi and Bluetooth on one chip behind one
antenna, so a `journalctl` dump competes directly with the A2DP stream it is
being used to investigate. On the first boot with a persistent journal to
read, the ssh login landed at 60.4 s and the reported stutters at roughly
80 s, 97 s and 101 s — during the diagnosis, not merely near it. Some earlier
"stuttery audio" reports coincide with active diagnosis in the same way.

This is not the whole explanation — stutter was also reported on cycles with
nobody connected — but it is enough to invalidate any measurement taken over
ssh while listening. `vaino-startup-sample` exists for that reason: a boot-time
service that reads `/proc` once a second into a local file, with no network,
no subprocesses in its loop, `Nice=19` and idle I/O. It can run while the
listener simply listens, and be read afterwards with the appliance quiet.

**`[PI3-FOUND-180]` No audio thread on this appliance has real-time priority,
and the usual ways of granting it do not work here.** Measured: `pipewire`'s
`data-loop.0` — the thread that encodes SBC and feeds A2DP — runs
`SCHED_OTHER` at priority 0 carrying `SCHED_RESET_ON_FORK`, which is the
fingerprint of a request made and refused. `rtkit-daemon` confirms it, once a
minute: *"Supervising 0 threads of 0 processes of 0 users."* The cause is a
hard limit: `/proc/<pipewire>/limits` reads `Max realtime priority 0 0`.

Three fixes were tried and **none of them worked**, which is the part worth
recording:

- Adding the account to the `pipewire` group, the usual advice. Debian's
  `@pipewire - rtprio 95` is applied by `pam_limits`, and **`/etc/pam.d/systemd-user`
  does not exist on this image**, so no PAM limits reach a systemd *user*
  session — which is where PipeWire runs.
- `LimitRTPRIO=95` as a drop-in on `user@.service`. `systemctl show` duly
  reported 95 for the instance, and the manager process still came up with a
  hard limit of 0.
- `DefaultLimitRTPRIO=95` in `/etc/systemd/system.conf`, with `daemon-reexec`
  and across a full reboot. The manager default read 95; PID 1 itself and the
  user manager both still read 0.

All three were reverted rather than left in place, because configuration that
looks like it grants real time and does not is worse than none. Whether RT is
even the right target is unproven: `pw-top` shows the audio thread using 2–4%
of its quantum with zero xruns in steady state, so it has considerable headroom
and would need a >20 ms scheduling delay to glitch. Establish that such delays
actually occur before spending more on this.

**The upower remedy recorded above never survived a reboot, and this is
why.** `systemctl enable --now upower` worked — for that session.
`upower.service` ships `WantedBy=graphical.target`, and this appliance boots
to `multi-user.target` with no display, so enabling it creates a want that is
never reached. Every boot since has come up with `upower` *enabled* and
*inactive*, and WirePlumber has logged
`Failed to get percentage from UPower: org.freedesktop.DBus.Error.NameHasNoOwner`
each time. The verb that actually holds on a headless appliance is
`systemctl add-wants multi-user.target upower.service`, which is now in place.

Worth noting what this did **not** turn out to explain: the error appears
exactly once per boot here, at about 13 s, not on the repeating ~2.5 minute
sweep that `[PI3-FOUND-030]` originally described, and the startup stutter
being investigated happens at 63-80 s. Fixed because it is a known hazard and
the fix is one line, not because it was the cause.

**`[PI3-FOUND-190]` What the startup stutter is not.** With a sampler running
unattended (`vaino-startup-sample`, no ssh connected), the window the listener
reported as stuttering — 63 s to 80 s — shows: swap counters static, 216 MB
available, the player using about 5% of one core, and zero xruns. The load
average sits near 1.4 but is a lagging average decaying from the library index
build at 27-50 s, not live work. So it is not memory, not swap, not CPU
starvation, and not the player missing its ring.

What remains is the radio. Wi-Fi and Bluetooth share one chip and one antenna
on a Pi Zero 2W, DHCP completed at 56.9 s and the NetworkManager dispatcher ran
57-67 s — overlapping the start of the stutter exactly — and no amount of CPU
headroom prevents an antenna being used by something else. Since that cannot be
removed, the lever is margin: the graph was running a 1024-frame quantum, about
21 ms, so any interruption longer than that glitches. `default.clock.min-quantum`
is now 2048, which doubles the tolerance to about 46 ms, at a latency cost that
is meaningless for music with no synchronised display.

**`[PI3-FOUND-330]` It was written off as useless, deleted, and its removal
was the regression.** The cold boot after this change stuttered exactly as
before, so it was recorded as having fixed nothing and later removed as dead
configuration. **That test was confounded** — the appliance was still sitting
on the speaker `[PI3-FOUND-320]`, a fault large enough to hide whatever the
buffer was doing.

Deleting the drop-in brought the stuttering straight back, on a boot eighteen
inches clear of the speaker with everything else as it had been through two
clean power cycles. The only difference was the quantum: 2048 while clean,
1024 once removed. So both mattered and neither alone sufficed — moving the
box stops the antenna being detuned, and the wider quantum carries the stream
over what interference remains. 1024 frames, about 21 ms, is not enough here.
Restored, this time on evidence rather than on the absence of it.

> **The lesson is about the test, not the setting.** "Changed X, symptom
> persisted, therefore X did nothing" holds only when nothing else is broken.
> Here a second fault dominated the measurement, and the conclusion drawn from
> it survived long enough to be acted on.

**`[PI3-FOUND-200]` Some of the early silence was the player starving its own
ring, and every other instrument said the machine was fine.** (Read the
qualifier: this accounts for the *startup* underruns, not for the periodic
stuttering that outlasted them — that was `[PI3-FOUND-320]`, and underrun
counts turn out not to predict audibility at all.) After ruling out memory,
swap, CPU and PipeWire xruns, the measurement that had not been taken was the
player's own:

```
$ vaino-underruns
underrun_samples   654768  (14.85s at 44.1kHz)
lock_failures      1
```

Fourteen and a half seconds of samples the output ring could not supply, in a
startup where `pw-top` reported **zero** xruns, `free` showed 216 MB
available, the swap counters never moved, and the player used about 5% of one
core. None of those instruments could have shown it: from PipeWire's side
nothing went wrong — it was handed silence and delivered silence faithfully.

Sampled three times ten seconds apart afterwards, the counter did not move.
So the whole 14.85 s accrued during startup and stopped, which is exactly the
shape the listener reported: stuttering as playback begins, clean later.

**5% of a core while starving means blocked, not slow.** The player is not
compute-bound during that window; it is waiting. What it waits on is the SD
card, which has just been asked to read a 1.1 GB library database — flushing
the page cache and saturating the queue immediately before playback begins.
The player then had no explicit I/O class at all, only whatever its `Nice=-5`
implied, and 128 KB of readahead for sequential audio files.

Two changes, both cheap and both at the level the problem actually lives:
`IOSchedulingClass=best-effort` with `IOSchedulingPriority=0` on
`vaino.service`, and readahead raised to 512 KB through `tmpfiles.d`.

**Outcome, across the cold boots that followed:** startup underruns did fall
a long way — 654,768 samples to 118,346 — but attributing that to these two
changes would be wrong, because `mpd` was made to yield in the same window
`[PI3-FOUND-210]` and that is the change with a measured 19 s → 1.9 s behind
it. Worse, the process-wide I/O priority here is too blunt to be the right
shape: it raises the Director rebuild along with the decoder, which is the
opposite of what `[PI3-FOUND-220]` then had to arrange per-thread. Both are
kept as harmless and defensible, neither is evidence of anything.

The durable answer is almost certainly the one vainoplayer3 already has:
deferring the library/Director build off the resume path, so the card is not
being saturated in the seconds before audio starts. That needs a
cross-compiled binary rather than a configuration change.

`vaino-underruns` exists so this is never again invisible: the counter is
published only over the websocket, and nothing on the appliance could read it.

## 2. Where the startup time and the stutters actually went

**`[PI3-FOUND-210]` `Session::open` was never slow; it was starved.**
Instrumented and measured on the appliance, the phases read:

```
warm restart   session open: library 8ms, store 10ms, resume-load 0ms, utc-sync 1ms (total 19ms)
caches dropped session open: total 25ms,  prime 712ms
cold boot      session open: library 474ms ... (total 489ms), prime 690ms
```

Nineteen *milliseconds* of work had been taking nineteen *seconds* on a boot.
Nothing in the function was expensive; it was competing with everything else
starting at once on four small cores and one SD card, and the boot journal
shows a fifteen-second silence across the window in which nothing logged and
the machine was plainly busy. A sidecar to skip the library open — the
obvious fix, and one seriously considered — would have saved 19 ms.

The remedy was to stop the competition instead: `mpd`, a guest backend that
is configured but idle until switched to `[SPEC-BK-020]`, now runs
`IOSchedulingClass=idle` and `Nice=10`. On the next cold boot the whole
player startup was **1.9 s** (32.4 s to 34.3 s), and underruns fell from
654,768 samples to 102,266 — 14.85 s of missing audio to 2.32 s.

**`[PI3-FOUND-220]` The rest of the stutters were the Director rebuild,
which left no trace.** With the player itself starting in under two seconds,
the listener still heard stutters at 46 s, 61 s, 76 s, 89 s and 96 s. The
first two were the radio: `wpa_supplicant` re-associating, alternately with
two access points on the same SSID at near-identical strength (56 and 54, both
channel 1, no BSSID pinned), each attempt costing antenna time the A2DP link
needs. Those settled at 64 s and never recurred.

The last three were the Director rebuild — a flavor index over 8,330 radio
passages, **measured at 16.5 s and 10.3 s**, running at the same priority as
the thread feeding the speaker, and logging absolutely nothing. That silence
is why its stutters were attributed to the radio for most of an evening.

`RELOAD_MIN_QUEUE_MS` already decided *when* it may start and decided it
well. Nothing decided how hard it should push once running. It now calls
`step_aside()` first — `setpriority` and `ioprio_set`, both per-thread on
Linux, so the engine keeps everything it has and only the rebuild yields —
and reports its duration.

> **Verified 2026-09-08 by measurement, not by ear.** The rebuild thread was
> caught mid-run reading `nice=10 idle`, and the player's own underrun counter
> read **72,590 before a 10.3 s rebuild and 72,590 after it**: zero samples
> lost to work that used to be audible. `vaino-underruns` is what made that a
> number rather than an opinion.

**`[PI3-FOUND-230]` The keeper's own polling was audible.** Every
`bluetoothctl` invocation opens a D-Bus connection and enumerates the
adapter's objects, and `vaino-speaker` had grown to three or four of them per
tick — trust, connected-list, audio-sink, alias — against a daemon that is at
that moment carrying an A2DP stream. Measured on a boot where the player
started in 1.9 s and the Director rebuild had already been made polite: the
listener's stutters at 120 s and 151 s land on the timer's own ticks at 118 s
and 153 s.

All of it is in one `info` block, so it is fetched once and read several
times, and the connected-device search is skipped entirely when the speaker
on record is the one connected — the case that runs every thirty seconds
forever. A tick now costs 0.245 s rather than 0.4 s, and one round trip to
BlueZ rather than four. Verified against all three branches afterwards: trust
self-heal still repairs an untrusted speaker and persists it, a healthy tick
is silent, and a routing mismatch still asks the player to reopen.

**The Wi-Fi roam-flap is left alone, deliberately.** Two access points share
the SSID at near-identical strength and the client re-associates between
them, each attempt costing antenna time the A2DP link needs — the listener's
stutters at 46 s and 61 s on one boot were exactly this. Pinning a BSSID
would stop it, and was declined on purpose: *"accessibility is more important
than temporary radio instability"* — the appliance is headless and that Wi-Fi
is the only way to reach it, so a pin that outlives the AP it names would
cost far more than the stutters do. Recorded so the option is not
rediscovered and quietly taken later.

**`[PI3-FOUND-540]` The sink gate counted passes and called them seconds.**
Measured 2026-09-10, on a boot whose chosen speaker had been switched off:
`vaino.service` took 1 min 8 s, and the gate ran from 17:02:19 to 17:04:42
before printing *"no real sink after 60s"*. It had waited **143 seconds**.

The loop incremented a counter once per iteration and treated it as a second,
but each pass also runs `wpctl status`, which on a Pi Zero 2W during boot costs
well over a second by itself. So `DEADLINE=60` bought roughly 143 s of waiting,
and every comment in the file describing the deadline in seconds was wrong --
including the ones weighing 15 s against 45 s in `[PI3-FOUND-260]`, whose real
figures were larger by the same factor.

**What made it matter is where the gate sits.** It is an `ExecStartPre` for
`vaino.service`, so it does not merely delay audio -- it holds the web
interface down with it. On that boot the listener could not reach the page
that would have let them choose a speaker that was actually present, for the
entire time the appliance spent waiting for one that was not. A gate meant to
protect the audio had taken the controls away.

The deadline now measures elapsed wall-clock time, which is what the file
always claimed. The fast path is unchanged and still exits the moment the
speaker's sink appears -- verified on the appliance at 0 s with the speaker
present.

> **Worth revisiting separately.** Waiting is defensible while the chosen
> speaker is *coming*; it is pure loss when that speaker is switched off, and
> the appliance cannot tell the two apart. Nothing here changes that, and the
> interface stays gated behind the audio wait.

**`[PI3-FOUND-550]` The gate now waits five seconds, not sixty.** The 60 s
deadline came from `[PI3-FOUND-260]`, which measured that starting before the
speaker exists costs a reopen and 26 s of underrun while starting after costs
only silence that was silent anyway. That is still true about *audio*, and it
was the wrong thing to optimise: the gate is an `ExecStartPre`, so it was
buying quieter audio with the listener's access to the controls
`[PI3-FOUND-540]`. The listener's judgement settled it -- five seconds is
acceptable, 140 is not.

The fast path is unchanged: a speaker already present releases the gate
immediately, measured at 0 s. What changes is the failure case, which now
costs five seconds instead of over two minutes. The audio is no longer the
gate's problem; the keeper connects the speaker and asks the player to reopen.

**`[PI3-FOUND-560]` And a missing speaker no longer means silence.** Asked for
directly after the same boot: the appliance sat waiting on a speaker that had
been switched off while a second, known speaker was awake in the same room. It
had everything it needed to make sound and made none.

`vaino-speaker` now tries the other known speakers when the remembered one
does not answer. Deliberately narrow:

- Only after the chase has already failed, and only when `BUDGET` is non-zero
  -- which means nothing is currently audible. It can never interrupt playback
  to go hunting, which was the injury behind `[PI3-AIM-060]`.
- Only devices that are **paired and advertise an Audio Sink**. Something
  deliberately introduced to this appliance, never something merely in range.
- Bounded at 20 s total, three seconds per candidate.
- **It does not rewrite `speaker_address`.** The listener's choice still stands
  and is preferred again on the next boot. This is a stand-in for a missing
  speaker, not a new decision about which speaker this is.

**Trust was the obvious test and it was the wrong one.** The first version
required `Trusted: yes` and would have skipped exactly the speaker it existed
to reach: this appliance untrusts every speaker but the chosen one, so that
only the chosen one may reconnect to *us* unasked `[PI3-FOUND-130]`, and the
Middleton read `Trusted: no` the moment the listener switched to the Oontz.
Trust governs an inbound connection; this one is outbound, and a bond is what
says a device is known. Caught by checking the candidate list against the real
adapter rather than trusting the predicate: it returned one speaker where it
should have returned two.

**Exercised for real, 2026-09-10.** The Oontz -- the remembered speaker -- was
powered down while it was playing. The Middleton was untrusted, so nothing but
this path could have done it, and audio was playing from the Middleton about
forty seconds later with no intervention `[PI3-FOUND-590]`.

**The gate change measured, on the boot of 2026-09-10 17:16.** The journal
reads what it was built to read:

    17:16:28  vaino-wait-sink: no real sink after 5s; starting anyway
    17:16:28  Started vaino.service
    17:16:29  connected 08:EB:ED:26:14:12 after 6s and asked the player to reopen

`vaino.service` no longer appears in `systemd-analyze blame` at all, against
1 min 8 s on the boot that prompted this. The gate released at five seconds,
the keeper landed the speaker a second later and asked for the reopen, and the
listener heard clean audio at +36 with no stutters -- **so the reopen this
trade was expected to cost was not audible**. That is the `[PI3-FOUND-260]`
worry answered on its own terms rather than argued away.

**`[PI3-AIM-090]` One copy of each fact, in `vaino-common.sh`.** Reviewed
against the one-sentence policy of `[PI3-AIM-080]` on 2026-09-10. Five things
had been written out four and five times over:

| Duplicated | Copies | Now |
| --- | --- | --- |
| where the listener's database lives | 5 | `vaino_db` |
| reading `speaker_address` out of it | 4 | `vaino_speaker` |
| parsing a sink name from `wpctl status` | 4 | `vaino_sinks`, `vaino_sink_present` |
| a device's alias | 3 | `vaino_alias` |
| decoding an AFH map | 2, in three forms | `vaino_afh_channels`, `_bytes`, `_excluded` |

**Why it mattered rather than merely offended.** The sink parser was wrong
once, in a way that let the player open onto a dummy and report success
`[PI3-FOUND-110]`; the fix then had to be carried by hand into everything that
had copied it. And a helper reading the pre-split database tells the listener
about the speaker they used to have `[PI3-FOUND-280]` -- a failure that has
happened here, and the kind that only happens to duplicated knowledge.

Sourced, not executed:

    . "${VAINO_COMMON:-/usr/local/lib/vaino-common.sh}" 2>/dev/null ||
        . "$(dirname "$0")/vaino-common.sh"

**`[PI3-FOUND-650]` And one rule was deleted outright, not moved.**
`vaino-btctl`'s `withdraw_others` untrusted every speaker but the chosen one,
so none could let itself in over it. The agent now refuses any audio profile
from anything that is not holding the audio `[PI3-FOUND-630]`, which reaches
the same outcome by answering rather than by pre-emption -- and untrusting had
a sharp side effect: a paired but untrusted speaker, powered on nearby, was
refused for want of an agent and knocked every nine seconds for as long as it
stayed on `[PI3-FOUND-610]`.

It had also forced a subtlety on the keeper's fallback, which tested `Paired`
rather than `Trusted` precisely because this code untrusted the speaker it
needed to reach `[PI3-FOUND-560]`. **One rule removed, one gotcha removed with
it, and the thing it was protecting against still handled.**

Net across the review: `vaino-speaker.sh` 503 → 340 lines, `vaino-afh-seed`
137 → 108, `vaino-linkstate` 102 → 93, `vaino-wait-sink` 145 → 130, against 137
lines of shared library that replaced roughly twice that in copies.

**`vaino-bt-agent selftest` — the one test this appliance ships.** Checks
every Bluetooth audio and control profile against both an incumbent caller and
an intruder, with and without an incumbent recorded: 36 cases, no D-Bus, no
effect on the running agent.

    vaino-bt-agent selftest

Its UUIDs are transcribed by hand from the assigned-numbers list rather than
read from the code they check, because the version that read from the code
passed five cases while the agent waved through the profile BlueZ actually
asks about `[PI3-FOUND-660]`. **A test that shares the implementation's blind
spot tests nothing.** Verified by reintroducing that exact bug and confirming
the test fails on it.

**`[PI3-FOUND-670]` The keeper stopped running, and nothing said so.** The
listener powered down the speaker holding the audio. Four minutes later
nothing had reconnected, the incumbent file was stale, and the stream sat on
`Dummy Output` -- while the same work run by hand recovered the appliance in
25 seconds. **The appliance was not slow to recover. It was not running.**

`systemctl` showed the service stuck in `activating`, and the cause is a
default worth knowing: **systemd sets `TimeoutStartSec` to infinity for
`Type=oneshot`.** A tick that blocks inside `bluetoothctl` is therefore never
killed, and a timer cannot fire while the previous tick is still running -- so
one blocked call silently stops every future tick. Every layer looked healthy:
the timer was `active`, the service had not failed, and nothing was logged,
because a process that is stuck logs nothing.

Two fixes, because the budgets and the timeout answer different failures:

- `TimeoutStartSec=45` on the service. systemd now kills an overrunning tick
  and the next one proceeds. This is the one that matters -- no arrangement of
  internal deadlines can bound a single call that never returns.
- One budget for the tick, shared: 15 s for the chase and the remainder for
  the fallback, 25 s total against a 30 s period. Previously they were
  independent -- 22 s and 20 s -- so a healthy tick with the chosen speaker
  switched off could legitimately want 42 s in a service fired every 30. The
  two shares stay separate rather than sharing one deadline, because a chosen
  speaker that is switched off would otherwise eat the whole tick every time
  and the fallback would never run.

A settled tick costs 0 s, measured; only the absent case spends anything.

## 3. Diagnostic tools, and how to switch them back on

Seven tools were built during the 2026-09-08 investigation and the stutter
hunt that followed -- six instruments and one intervention. Four are **off by
default** because they cost something to run or because they change behaviour;
all seven stay installed, because the expensive part was working out what to
measure, not writing it.

**`vaino-underruns` — always available, costs nothing.** Prints the player's
own `underrun_samples`: how many samples the output ring failed to supply.
The single most useful number here, and the one that finally separated "the
player could not keep its buffer fed" from "the radio dropped packets" —
PipeWire reports zero xruns for the first case, because from its side nothing
went wrong `[PI3-FOUND-200]`. Run it twice a few seconds apart: a counter that
is still climbing is a live fault, one that has stopped is a startup
transient.

**`vaino-startup-sample` — installed, disabled.** A boot service that reads
`/proc` once a second into `/var/log/vaino-startup.log`: uptime, load, the
player's CPU, and its `read_bytes`/`write_bytes`/`rchar` — block-device
traffic separated from reads the page cache served. Built because diagnosing
over ssh perturbs what it measures `[PI3-FOUND-170]`, and it answered two
questions: that the startup stutter was not memory, swap or CPU, and that
after the first ninety seconds the decoder touches the card not at all.

**`[PI3-FOUND-240]` Its first version was a suspect in its own
measurements.** It walked every process in `/proc` on each pass to find the
player, which cost about 5% of a core, so it could not be ruled out of the
stutters it was watching for. Disabling it left them unchanged at their
steady fifteen-second spacing, which cleared it — but an instrument that has
to be alibied is a bad instrument. It now finds the player once and spends
one subprocess a second, so its cost is constant rather than periodic and
cannot produce a periodic symptom.

Still disabled by default — an idle appliance should not be paying for an
instrument nobody is reading.

    sudo systemctl enable --now vaino-startup-sample    # on
    sudo systemctl disable --now vaino-startup-sample   # off

**`vaino-hci-capture` — installed, disabled, and not yet read.** Counts what
reaches the air. `btmon` is filtered down to two line types -- the ACL data
packets handed to the controller, and the `Number of Completed Packets` flow
control coming back -- and the result is one line a second: monotonic clock,
packets out, completions in, bytes. Healthy playback measures about 75
packets/s.

It exists because nothing else here detects the symptom `[PI3-FOUND-420]`.
The underrun counter does not correlate, PipeWire reports zero xruns either
way, and a capture off the sink monitor is taken before the SBC encoder and is
always continuous. Every other instrument sits before the loss; HCI is the one
layer left between the encoder and the antenna. A dip or a gap during an
audible stutter means the packets are not getting out. A rate that stays flat
means the loss is past the controller, inside the speaker.

Bounded on purpose: a fixed window (`VAINO_HCI_SECS`, default 180 s), grep
applied before a byte is stored, the raw stream held in tmpfs so the card sees
none of it, and the per-second aggregation done only once the window has
closed, so no analysis competes with the audio it is watching. This
investigation has been misled by its own instruments twice
`[PI3-FOUND-170]`, `[PI3-FOUND-240]`, and `btmon` is the most invasive one
yet.

**Verified on the appliance, against settled playback, 2026-09-10.** Twenty
seconds of healthy audio to the Middleton measured 75-77 packets/s at 612
bytes each, dead flat, with completions running at half the transmit rate --
dead flat. That is the baseline a stuttering boot has to be compared against,
and on the same day it was compared: a stutter train measured 61 pkt/s against
it, a 17% shortfall, ending at the second the listener said the stutters
stopped `[PI3-FOUND-430]`. The completions column has an unexplained anomaly
in that run and should not be relied on yet.

    # boot ... -- hci capture, 20s window
    # mono acl_tx completed bytes
    861.34 75 38 45900
    862.34 75 38 45900
    863.35 76 37 46512

    sudo systemctl start vaino-hci-capture              # one window, this boot
    sudo systemctl enable vaino-hci-capture             # and on the next boot
    cat /var/log/vaino-hci.log                          # after the window closes

**`vaino-linkstate` — always available, read-only, about a second.** One shot
of everything Bluetooth will say about the link and the speaker at the far end
of it: ACL role and direction as *separate* fields, the AFH hop map with a
channel count, link quality, RSSI, transmit power, supervision timeout, and
the transport's state, AVRCP volume and SBC configuration. Run it right after
a boot in each power-cycle mode and diff the two.

It exists because `vaino-hci-capture` says only whether audio is getting out,
not what the link looks like while it is not. The two fields it was built for
are `role`, which had never been read as distinct from direction, and `afh`,
which had never been read at all `[PI3-FOUND-490]`.

    sudo vaino-linkstate

**`vaino-afh-seed` — installed, disabled, and the only one that changes
behaviour.** Not an instrument: it hands the controller a channel
classification saved from a settled link, so a boot starts adapted instead of
learning the room again. `save` captures the live map, `show` decodes it into
MHz, `apply` writes it, `boot` applies it seven times across the window in
which the link comes up. Built to test `[PI3-FOUND-520]`, and it comes back
out if that test fails.

Everything else here only watches. This one acts, and it encodes an assumption
about one room, so it is the one to disable first when something is behaving
strangely.

    sudo vaino-afh-seed save                       # while it sounds right
    sudo systemctl enable --now vaino-afh-seed     # on
    sudo systemctl disable --now vaino-afh-seed    # off

**`[PI3-FOUND-620]` `vaino-vitals` — installed, disabled, and the answer to a
record that died with the fault.** On 2026-09-10 the appliance stopped
answering TCP for thirteen minutes while still replying to ping. The journal
simply ends mid-sequence: no I/O error, no OOM, no hung task, no panic --
because journald is userspace too and stopped with everything else
`[PI3-FOUND-610]`.

So this samples to a plain file on a fixed cadence, each line complete in
itself, and **the gap where the lines stop is as informative as the lines**:

    # mono load1 load5 run/total memavail_kB swapfree_kB iowait_d listen22 listen5720
    212.42 0.26 0.35 1/190 270192 516928 11 1 1
    217.44 0.24 0.34 1/190 271188 516928  2 1 1

`load` rising against an idle CPU means blocked rather than busy; `iowait_d` is
jiffies waiting on I/O since the last sample, which separates a stalled card
from a busy one; and `listen22`/`listen5720` going 1 to 0 while the processes
still exist would *be* the wedge, named. That last field is precisely what was
not known during the incident.

**It forks nothing.** Five small `/proc` reads per sample, in shell. The
earlier sampler walked every process each pass, cost about 5% of a core, and
had to be alibied out of the symptoms it was watching `[PI3-FOUND-240]`; this
cannot produce a periodic symptom and never needs an alibi.

Off by default and meant to be switched off for production -- it writes to the
card every ten seconds, which an appliance that plays music should not do
forever.

    sudo systemctl enable --now vaino-vitals    # on
    sudo systemctl disable --now vaino-vitals   # off
    cat /var/log/vaino-vitals.log

**Persistent journal — off, restored to `Storage=volatile`.** The appliance
ships volatile deliberately: it is power-cut on every shutdown
`[PI3-FOUND-120]` and SD writes are not free. But volatile means a power
cycle destroys the evidence of what just went wrong, which is exactly the
class of fault this machine has. Three power cycles were investigated blind
before it was turned on, and it immediately paid for itself. Turn it on for
any mystery that survives a reboot, and off again afterwards:

    sudo sed -i 's/^Storage=volatile/Storage=persistent/' /etc/systemd/journald.conf
    sudo systemctl restart systemd-journald
    # and to revert, the same substitution the other way round

It also captures the *user* journal, where WirePlumber logs live — a blind
spot named in `[PI3-FOUND-030]`'s original investigation and not closed until
now.

