#!/usr/bin/env python3
"""Compare audio_equivalence exports from two builds; exact PCM is the default."""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import sys

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("before", type=Path)
parser.add_argument("after", type=Path)
parser.add_argument("--report", type=Path, required=True)
args = parser.parse_args()
before = {row["file"]: row for row in json.loads((args.before / "results.json").read_text())}
after = {row["file"]: row for row in json.loads((args.after / "results.json").read_text())}
if not before or before.keys() != after.keys():
    raise SystemExit("nonempty matching case sets required")
rows = []
for name in sorted(before):
    raw_a, raw_b = (args.before / name).read_bytes(), (args.after / name).read_bytes()
    if not raw_a or len(raw_a) != len(raw_b) or len(raw_a) % 2:
        raise SystemExit(f"invalid or different PCM duration: {name}")
    a, b = array.array("h"), array.array("h")
    a.frombytes(raw_a)
    b.frombytes(raw_b)
    if sys.byteorder != "little":
        a.byteswap()
        b.byteswap()
    differences = [int(x) - int(y) for x, y in zip(a, b)]
    error = sum(d * d for d in differences)
    power = sum(int(x) * int(x) for x in a)
    rows.append({"file": name, "samples": len(a), "identical": raw_a == raw_b,
                 "different_samples": sum(d != 0 for d in differences),
                 "max_error_lsb": max(map(abs, differences)),
                 "rms_error_lsb": math.sqrt(error / len(a)),
                 "snr_db": 10 * math.log10(power / error) if power and error else None,
                 "before_sha256": hashlib.sha256(raw_a).hexdigest(),
                 "after_sha256": hashlib.sha256(raw_b).hexdigest(),
                 "before_seconds": before[name]["seconds"],
                 "after_seconds": after[name]["seconds"]})
report = {"cases": len(rows), "identical_cases": sum(r["identical"] for r in rows),
          "max_error_lsb": max(r["max_error_lsb"] for r in rows), "results": rows}
args.report.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({k: v for k, v in report.items() if k != "results"}))
raise SystemExit(0 if report["identical_cases"] == report["cases"] else 1)
