"""Apply a Gaia chain to lowlevel features, and check it against the reference.

Route 2 `[GDE-FEX-067]`. The predictor and its verification are one tool on
purpose: every number this produces is compared against AcousticBrainz's own
published output for the same recording before it is believed `[GDE-FEX-090]`.
A transform chain that is subtly wrong still emits plausible probabilities, so
"it ran" is not evidence of anything.

Usage:
  python tools/gaia_predict.py <classifier> [n]     # verify against the pairs
"""

from __future__ import annotations

import json
import math
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import gaia_history as gh  # noqa: E402

SVM_DIR = Path("data/essentia/svm/essentia-extractor-svm_models-v2.1_beta5")
LOWLEVEL_DIR = Path("data/essentia/ab_lowlevel")
REFERENCE_DB = Path("data/flavor-sample.db")


# ------------------------------------------------------------------ features


def flatten(doc: dict) -> dict[str, list[float]]:
    """Lowlevel JSON to Gaia descriptor names.

    Gaia addresses features as `.lowlevel.spectral_decrease.mean`, which is the
    JSON path with a leading dot. Every value becomes a list so scalars and
    vectors are handled identically — `.tonal.hpcp.mean` is 36 numbers and
    `.tonal.tuning_frequency` is one.
    """
    out: dict[str, list[float]] = {}

    def walk(node, path: str) -> None:
        if isinstance(node, dict):
            for k, v in node.items():
                walk(v, f"{path}.{k}")
        elif isinstance(node, (int, float)) and not isinstance(node, bool):
            out[path] = [float(node)]
        elif isinstance(node, list) and node and all(
            isinstance(x, (int, float)) and not isinstance(x, bool) for x in node
        ):
            out[path] = [float(x) for x in node]

    walk(doc, "")
    return out


def build_vector(features: dict[str, list[float]], coeffs: dict, order: list[str]) -> list[float]:
    """Normalised feature vector, in `order`.

    Normalisation is `y = a·x + b` per component `[GDE-FEX-069]`. A descriptor
    the file lacks contributes zeros rather than aborting: partial input is a
    real case, and the verification will show if it matters.
    """
    vec: list[float] = []
    for name in order:
        c = coeffs[name]
        a, b = c["a"], c["b"]
        xs = features.get(name)
        for i in range(len(a)):
            x = xs[i] if xs and i < len(xs) else 0.0
            vec.append(a[i] * x + b[i])
    return vec


# ----------------------------------------------------------------- the model


class SvmModel:
    """A libsvm model, parsed from the text form Gaia stores."""

    def __init__(self, text: str):
        self.kernel = "rbf"
        self.gamma = 0.0
        self.degree = 3
        self.coef0 = 0.0
        self.nr_class = 2
        self.rho: list[float] = []
        self.label: list[int] = []
        self.probA: list[float] = []
        self.probB: list[float] = []
        self.nr_sv: list[int] = []
        self.sv_coef: list[list[float]] = []
        self.sv: list[dict[int, float]] = []

        lines = text.splitlines()
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            i += 1
            if line == "SV":
                break
            if not line:
                continue
            key, _, rest = line.partition(" ")
            vals = rest.split()
            if key == "kernel_type":
                self.kernel = vals[0]
            elif key == "gamma":
                self.gamma = float(vals[0])
            elif key == "degree":
                self.degree = int(vals[0])
            elif key == "coef0":
                self.coef0 = float(vals[0])
            elif key == "nr_class":
                self.nr_class = int(vals[0])
            elif key == "rho":
                self.rho = [float(v) for v in vals]
            elif key == "label":
                self.label = [int(v) for v in vals]
            elif key == "probA":
                self.probA = [float(v) for v in vals]
            elif key == "probB":
                self.probB = [float(v) for v in vals]
            elif key == "nr_sv":
                self.nr_sv = [int(v) for v in vals]

        ncoef = self.nr_class - 1
        for line in lines[i:]:
            parts = line.split()
            if not parts:
                continue
            self.sv_coef.append([float(x) for x in parts[:ncoef]])
            self.sv.append(
                {int(k): float(v) for k, v in (p.split(":") for p in parts[ncoef:])}
            )

    def kernel_value(self, sv: dict[int, float], x: list[float]) -> float:
        if self.kernel == "rbf":
            # ||u-v||^2 over the union of indices; x is dense, sv is sparse.
            s = 0.0
            seen = set()
            for k, v in sv.items():
                xv = x[k - 1] if k - 1 < len(x) else 0.0
                s += (v - xv) * (v - xv)
                seen.add(k)
            for k in range(1, len(x) + 1):
                if k not in seen:
                    s += x[k - 1] * x[k - 1]
            return math.exp(-self.gamma * s)
        # polynomial: danceability alone uses this [GDE-FEX-068]
        dot = sum(v * (x[k - 1] if k - 1 < len(x) else 0.0) for k, v in sv.items())
        return (self.gamma * dot + self.coef0) ** self.degree

    def decision_binary(self, x: list[float]) -> float:
        k = [self.kernel_value(sv, x) for sv in self.sv]
        return sum(c[0] * kv for c, kv in zip(self.sv_coef, k)) - self.rho[0]

    def probability_binary(self, x: list[float]) -> dict[int, float]:
        """P(label) via libsvm's Platt sigmoid, matching `sigmoid_predict`."""
        d = self.decision_binary(x)
        f = d * self.probA[0] + self.probB[0]
        p0 = math.exp(-f) / (1.0 + math.exp(-f)) if f >= 0 else 1.0 / (1.0 + math.exp(f))
        return {self.label[0]: p0, self.label[1]: 1.0 - p0}


# ---------------------------------------------------------------- comparison


def reference(mbid: str, characteristic: str) -> dict[int, dict[str, float]]:
    """Published values per submission: {submission: {class: value}}."""
    con = sqlite3.connect(REFERENCE_DB)
    out: dict[int, dict[str, float]] = {}
    for cls, val, sub in con.execute(
        "SELECT class, value, submission FROM flavor WHERE recording_mbid=? AND characteristic=?",
        (mbid, characteristic),
    ):
        out.setdefault(sub, {})[cls] = val
    return out


def verify(classifier: str, limit: int = 8) -> None:
    hist = SVM_DIR / f"{classifier}.history"
    coeff_steps = gh.normalize_coeffs(hist)
    # The last normalize is the one immediately before the SVM, so its output
    # is the space the support vectors live in.
    coeffs = coeff_steps[-1]
    model = SvmModel(gh.extract_svm_model(hist))
    print(f"{classifier}: {len(coeffs)} descriptors, kernel {model.kernel}, "
          f"{len(model.sv)} SVs, classes {model.label}")

    orders = {
        "as-read": list(coeffs.keys()),
        "sorted": sorted(coeffs.keys()),
        "reversed": list(reversed(list(coeffs.keys()))),
    }
    dims = sum(len(v["a"]) for v in coeffs.values())
    max_idx = max(max(sv.keys()) for sv in model.sv)
    print(f"vector dimensions {dims}, highest SV index {max_idx}")

    files = sorted(LOWLEVEL_DIR.glob("*.json"))[:limit]
    for label, order in orders.items():
        errs: list[float] = []
        for f in files:
            doc = json.load(open(f, encoding="utf-8"))
            x = build_vector(flatten(doc), coeffs, order)
            probs = model.probability_binary(x)
            ref = reference(f.stem, classifier)
            if not ref:
                continue
            # Two unknowns are resolved by taking the best case, deliberately:
            # which SUBMISSION the lowlevel file is [GDE-FEX-090a], and which
            # model label maps to which class name. Taking the best is the
            # generous reading -- if even that is far off, the chain is wrong,
            # which is the question being asked.
            names = sorted(next(iter(ref.values())).keys())
            candidates = [probs[model.label[0]], probs[model.label[1]]]
            errs.append(
                min(
                    abs(cls[names[0]] - p)
                    for cls in ref.values()
                    for p in candidates
                )
            )
        if errs:
            errs.sort()
            print(f"  order {label:<9} n={len(errs):>3}  median abs err {errs[len(errs)//2]:.4f}"
                  f"  best {errs[0]:.4f}  worst {errs[-1]:.4f}")


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    verify(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 8)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
