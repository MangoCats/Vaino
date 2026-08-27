# Fade curve fixture

One shared table of `(t, expected gain)` pairs for the exponential ramp
`fade.rs`'s `Curve::Exponential` implements and `[SPEC-AUD-040]` specifies.
Two independent implementations read it — Rust's `Fade` (real playback) and
the waveform editor's `fade.js` (the preview a person drags against,
`[SPEC021 §4]`) — so "how loud, here" is one number, computed twice and
checked to agree, rather than two formulas that can drift apart silently.

`exponential.json` is hand-computed from the closed form in `fade.rs`'s own
doc comment: `gain_in(t) = (10^(-60·(1-t)/20) - floor) / (1 - floor)`,
`floor = 10^(-60/20)`, `gain_out(t) = gain_in(1-t)`. Regenerate with:

```
python3 -c "
floor = 10**(-60/20)
def gain_in(t): return max(0, min(1, (10**(-60*(1-t)/20) - floor) / (1 - floor)))
def gain_out(t): return gain_in(1 - t)
for t in [0,.1,.2,.25,.3,.4,.5,.6,.7,.75,.8,.9,1]:
    print(f'{t:.2f}  {gain_in(t):.9f}  {gain_out(t):.9f}')
"
```

Checked by `player/src/fade.rs`'s `matches_the_shared_fade_fixture...` test
and by `build/verify-skins.js`'s fade check, which loads `player/src/web/fade.js`
directly with `require()` — it needs no DOM, so this is the one part of the
editor's math a Node process can verify without a real browser's Web Audio.
