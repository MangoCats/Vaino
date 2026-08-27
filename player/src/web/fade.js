// The exponential ramp curve, ported from `fade.rs`'s `Curve::Exponential`
// `[SPEC-AUD-040]`. Checked against `fixtures/fade/exponential.json`, the
// same table `fade.rs`'s own tests check against -- one formula, computed
// twice, kept from drifting apart silently `[SPEC021 §4]`.
//
// Kept in its own file rather than folded into `edit.js` so the check that
// verifies it can load exactly this and nothing else -- including from plain
// Node, with no DOM and no Web Audio, which is otherwise unreachable here.
(function (root) {
  const DEPTH_DB = 60;
  const FLOOR = Math.pow(10, -DEPTH_DB / 20);

  // Fade-**in** at normalised progress `t`, clamped to [0, 1]. Silent at
  // t=0, unity at t=1, for every curve -- see `fade.rs`'s own doc comment.
  function gainIn(t) {
    t = Math.max(0, Math.min(1, t));
    const raw = Math.pow(10, (-DEPTH_DB * (1 - t)) / 20);
    return Math.max(0, Math.min(1, (raw - FLOOR) / (1 - FLOOR)));
  }

  // Fade-**out** is the mirror of fade-in, which is why one function serves
  // both directions in `fade.rs` and does here too.
  function gainOut(t) {
    return gainIn(1 - t);
  }

  const api = { gainIn, gainOut };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  if (root) root.VainoFade = api;
})(typeof window !== 'undefined' ? window : undefined);
