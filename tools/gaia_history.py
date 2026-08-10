"""Read Gaia .history transformation chains [GDE-FEX-065] route 2.

A .history file is a Qt QDataStream serialisation of the transformation chain
AcousticBrainz applied between lowlevel features and a highlevel verdict:
remove -> select -> normalize -> (PCA) -> SVM. Reimplementing that chain is the
only route to the six complex characteristics that needs no training and is
exactly verifiable -- run it on AcousticBrainz's own lowlevel input and compare
against AcousticBrainz's own published highlevel output [GDE-FEX-090].

This module reads the structure. It does not yet apply it.

Usage:  python tools/gaia_history.py <file.history> [--verbose]
        python tools/gaia_history.py --survey <dir>
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

MAGIC = 0x6AEA723D


class Reader:
    """QDataStream primitives. Qt writes big-endian by default."""

    def __init__(self, data: bytes):
        self.d = data
        self.p = 0

    def remaining(self) -> int:
        return len(self.d) - self.p

    def u8(self) -> int:
        v = self.d[self.p]
        self.p += 1
        return v

    def i32(self) -> int:
        v = struct.unpack_from(">i", self.d, self.p)[0]
        self.p += 4
        return v

    def u32(self) -> int:
        v = struct.unpack_from(">I", self.d, self.p)[0]
        self.p += 4
        return v

    def f64(self) -> float:
        v = struct.unpack_from(">d", self.d, self.p)[0]
        self.p += 8
        return v

    def qstring(self) -> str | None:
        """qint32 byte length then UTF-16BE. 0xFFFFFFFF is a null string."""
        n = self.u32()
        if n == 0xFFFFFFFF:
            return None
        if n > self.remaining():
            raise ValueError(f"string length {n} exceeds {self.remaining()} remaining")
        s = self.d[self.p : self.p + n].decode("utf-16-be", errors="replace")
        self.p += n
        return s

    def qstringlist(self) -> list[str]:
        return [self.qstring() for _ in range(self.u32())]


def parse(path: Path, verbose: bool = False) -> dict:
    """Read the chain far enough to name every step and its descriptors."""
    r = Reader(path.read_bytes())
    magic = r.u32()
    if magic != MAGIC:
        raise ValueError(f"not a Gaia history: magic {magic:#x}")

    info: dict = {"file": path.name, "magic": magic, "header": [r.i32() for _ in range(3)]}
    steps: list[dict] = []

    # Sweep every offset for a length-prefixed UTF-16BE string. Modelling
    # Gaia's parameter tree is what APPLYING the chain will need; the question
    # here is only whether the chain is legible, and the embedded strings answer
    # it -- they name every step and every descriptor the step operates on.
    data = r.d
    p = 4
    end = len(data)
    while p + 4 < end:
        n = struct.unpack_from(">I", data, p)[0]
        # Plausible QString: non-empty, even byte count, fits, bounded length.
        if 2 <= n <= 400 and n % 2 == 0 and p + 4 + n <= end:
            raw = data[p + 4 : p + 4 + n]
            # UTF-16BE ASCII has a zero high byte in every code unit.
            if all(raw[i] == 0 for i in range(0, n, 2)) and all(
                32 <= raw[i] < 127 for i in range(1, n, 2)
            ):
                s = raw.decode("utf-16-be")
                steps.append({"name": s, "offset": p})
                if verbose:
                    print(f"  {p:>10}  {s}")
                p += 4 + n
                continue
        p += 1

    info["steps"] = steps
    names: list[str] = []
    for s in steps:
        if s["name"] not in names:
            names.append(s["name"])
    info["step_kinds"] = names
    return info


# Qt QMetaType ids, as QDataStream writes them.
T_BOOL, T_INT, T_UINT, T_DOUBLE = 1, 2, 3, 6
T_MAP, T_LIST, T_STRING, T_STRINGLIST = 8, 9, 10, 11


def read_variant(r: Reader, depth: int = 0):
    """One QVariant: quint32 type, quint8 isNull, then the payload.

    Only the types Gaia's parameter tree actually uses. An unknown type raises
    rather than guessing — a wrong guess here would produce plausible numbers,
    which is the worst possible failure for a transform chain.
    """
    if depth > 24:
        raise ValueError("variant nesting too deep")
    t = r.u32()
    is_null = r.u8()
    if is_null:
        return None
    if t == T_MAP:
        n = r.u32()
        return {r.qstring(): read_variant(r, depth + 1) for _ in range(n)}
    if t == T_LIST:
        return [read_variant(r, depth + 1) for _ in range(r.u32())]
    if t == T_STRING:
        return r.qstring()
    if t == T_STRINGLIST:
        return r.qstringlist()
    if t == T_DOUBLE:
        return r.f64()
    if t in (T_INT, T_UINT):
        return r.i32()
    if t == T_BOOL:
        return bool(r.u8())
    raise ValueError(f"unhandled QVariant type {t} at {r.p}")


# NOTE: the record framing is only understood as far as the first step
# [GDE-FEX-070a]. Each record begins QString name, QString applier, then TWO
# QVariantMaps -- but after those comes an empty map and a region that could be
# a QByteArray of applier state or a mis-framed string, and choosing between
# them by inspection is guesswork. A sequential reader built on a guess would
# associate parameters with the wrong steps and still produce numbers that look
# fine. So the extractors below locate parameters BY NAME and occurrence, and
# the 658-pair verification is what confirms the association is right.


def read_param_at(path: Path, name: str, occurrence: int = 0):
    """Read the QVariant that follows the parameter named `name`.

    Locating by name rather than walking the whole chain from the start: the
    chain's outer framing is not yet modelled, and the parameters themselves are
    self-describing once found.
    """
    data = path.read_bytes()
    key = name.encode("utf-16-be")
    needle = struct.pack(">I", len(key)) + key
    pos = -1
    for _ in range(occurrence + 1):
        pos = data.find(needle, pos + 1)
        if pos < 0:
            return None
    r = Reader(data)
    r.p = pos + 4 + len(key)
    return read_variant(r)


def normalize_coeffs(path: Path) -> list[dict[str, dict]]:
    """Every `normalize` step's per-descriptor `a` and `b` vectors, in order.

    A **list**, not a single map, because six of the eighteen chains normalize
    twice — once at step five and again before the SVM. Returning "the" coeffs
    would silently hand back the wrong step for a third of the classifiers, and
    the result would still look like plausible numbers.

    Normalisation is `y = a·x + b`: `tuning_frequency` carries `a=0.0402,
    b=-17.35`, mapping 440 Hz to 0.35 — a min-max scaling written as a scale
    and an offset rather than as a range.
    """
    out: list[dict[str, dict]] = []
    for i in range(8):
        c = read_param_at(path, "coeffs", occurrence=i)
        if c is None:
            break
        out.append(c)
    return out


def extract_svm_model(path: Path) -> str | None:
    """The libsvm model, as text.

    Gaia stores it under the `modelData` parameter as the *contents of a libsvm
    model file* — the documented text format, not a bespoke serialisation. So
    the hardest-looking part of route 2 needs no reverse engineering at all:
    find it and read forward while the bytes stay printable.
    """
    d = path.read_bytes()
    i = d.find(b"svm_type")
    if i < 0:
        return None
    j = i
    n = len(d)
    while j < n and (32 <= d[j] < 127 or d[j] in (9, 10, 13)):
        j += 1
    return d[i:j].decode("ascii")


def svm_summary(model: str) -> dict:
    """Header fields of a libsvm model. Everything after `SV` is data."""
    out: dict = {}
    for line in model.splitlines():
        if line.startswith("SV"):
            break
        parts = line.split()
        if len(parts) >= 2:
            out[parts[0]] = " ".join(parts[1:])
    out["sv_lines"] = sum(1 for _ in model.splitlines()) - len(out) - 1
    return out


KNOWN = {
    "remove",
    "select",
    "normalize",
    "pca",
    "svmtrain",
    "gaiatransform",
    "fixlength",
    "cleaner",
    "removevl",
    "enumerate",
}


def survey(directory: Path) -> None:
    files = sorted(directory.glob("*.history"))
    if not files:
        print(f"no .history files in {directory}")
        return
    print(f"{'classifier':<22} {'MB':>5} {'steps':>6}  kinds")
    total_ok = 0
    for f in files:
        try:
            info = parse(f)
        except Exception as e:  # noqa: BLE001 - a survey reports failures
            print(f"{f.stem:<22} {'':>5} {'':>6}  FAILED: {e}")
            continue
        total_ok += 1
        kinds = [k for k in info["step_kinds"] if k.lower() in KNOWN]
        print(
            f"{f.stem:<22} {f.stat().st_size / 1e6:>5.1f} "
            f"{len(info['steps']):>6}  {', '.join(kinds[:6])}"
        )
    print(f"\n{total_ok}/{len(files)} parsed")


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    if args[0] == "--survey":
        survey(Path(args[1]))
        return 0
    info = parse(Path(args[0]), verbose="--verbose" in args)
    print(f"{info['file']}: header {info['header']}, {len(info['steps'])} step markers")
    print("kinds:", ", ".join(info["step_kinds"][:20]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
