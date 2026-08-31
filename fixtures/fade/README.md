# Fade curve fixtures

One shared table of `(t, expected gain)` pairs per curve `fade.rs`'s `Curve`
implements and `[SPEC-AUD-040]` specifies — `exponential.json`, `linear.json`,
`cosine.json`. Two independent implementations read them — Rust's `Fade`
(real playback) and the waveform editor's `fade.js` (the preview a person
drags against, `[SPEC021 §4]`, `[SPEC-SUI-226]`) — so "how loud, here" is one
number per curve, computed twice and checked to agree, rather than three
formulas (one per curve) that can drift apart silently.

Hand-computed from the closed forms in `fade.rs`'s own doc comments,
`gain_out(t) = gain_in(1 - t)` for every curve:

```
python3 -c "
import math
floor = 10**(-60/20)
curves = {
    'linear':      (lambda t: t,
                     lambda t: 1 - t),
    'cosine':      (lambda t: (1 - math.cos(math.pi * t)) * 0.5,
                     lambda t: (1 - math.cos(math.pi * (1 - t))) * 0.5),
    'exponential': (lambda t: max(0, min(1, (10**(-60*(1-t)/20) - floor) / (1 - floor))),
                     lambda t: max(0, min(1, (10**(-60*t/20) - floor) / (1 - floor)))),
}
for name, (gain_in, gain_out) in curves.items():
    print(name)
    for t in [0,.1,.2,.25,.3,.4,.5,.6,.7,.75,.8,.9,1]:
        print(f'  {t:.2f}  {gain_in(t):.9f}  {gain_out(t):.9f}')
"
```

Checked by `player/src/fade.rs`'s `matches_the_shared_fade_fixture...` test
(one curve per fixture file) and by `build/verify-skins.js`'s fade check,
which loads `player/src/web/fade.js` directly with `require()` — it needs no
DOM, so this is the one part of the editor's math a Node process can verify
without a real browser's Web Audio.
