#!/usr/bin/env python3
"""Record and compare Indent's benchmark numbers against a committed baseline.

Phase 0 of the memory plan asks for exactly this, and says why:

    Run each with `hyperfine --warmup 5 --runs 20`, store results in a JSON
    file, and diff that file after every phase. **If a phase doesn't move the
    number, revert it.** Do not keep changes "because they look right."

`benchmarks/verify.sh` claimed to "measure performance against the recorded
baseline", but nothing was ever recorded — every run printed numbers and threw
them away, so a change that cost 20% was invisible unless someone remembered the
old figure. This script is the record.

Usage:
    python3 benchmarks/check_baselines.py                 # compare (5 runs each)
    python3 benchmarks/check_baselines.py --update        # re-record
    python3 benchmarks/check_baselines.py --runs 3 --tolerance 20
    python3 benchmarks/check_baselines.py --binary ./indent-native/target/release/indent

Each case is the best (minimum) value over the runs: the machine is noisy, and
the minimum is the estimator that ignores that noise instead of averaging it in.

Exit status is 1 when a case is slower than its baseline by more than the
tolerance, so this can gate a release. Improvements are reported but never fail.
"""
import argparse
import json
import os
import re
import subprocess
import sys
from datetime import date

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINE = os.path.join(ROOT, "benchmarks", "baselines.json")

# Only per-case numbers. The summary lines these files also print ("checks ...",
# "us/op over 200000 iterations each", "bench: DONE") are not measurements.
SKIP_PREFIXES = ("checks", "us/op over", "bench:", "DONE", "#")

# `<label>: <value> us/op` and `<label>: <value> s`
RE_UNIT = re.compile(r"^(?P<label>[^:]+?):\s+(?P<value>[0-9.]+)\s*(?P<unit>us/op|ns/op|ms/op|s)\b")
# `<label>  <value> ns/op` — no colon, the style `bench_do_catch.ind` uses
RE_BARE = re.compile(r"^(?P<label>.+?)\s\s+(?P<value>[0-9.]+)\s*(?P<unit>ns/op|us/op)\s*$")
# `<label>: <value>s total, <value> ns/step, len N` — record both costs
RE_TOTAL_STEP = re.compile(
    r"^(?P<label>[^:]+):\s+(?P<total>[0-9.]+)s total,\s+(?P<step>[0-9.]+)\s*ns/step"
)


def parse(text):
    """Pull (case, value) pairs out of one benchmark run."""
    found = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith(SKIP_PREFIXES):
            continue
        m = RE_TOTAL_STEP.match(line)
        if m:
            found[f"{m['label'].strip()} (total s)"] = float(m["total"])
            found[f"{m['label'].strip()} (ns/step)"] = float(m["step"])
            continue
        m = RE_UNIT.match(line) or RE_BARE.match(line)
        if not m:
            continue
        label = m["label"].strip()
        unit = m["unit"]
        found[f"{label} ({unit})"] = float(m["value"])
    return found


def run_case(binary, path, runs):
    """Best (minimum) value per case over `runs` executions of one file."""
    best = {}
    for _ in range(runs):
        proc = subprocess.run(
            [binary, path], cwd=ROOT, capture_output=True, text=True, errors="replace"
        )
        if proc.returncode != 0:
            print(f"  ! {os.path.basename(path)} exited {proc.returncode}; skipped")
            return None
        for case, value in parse(proc.stdout + proc.stderr).items():
            if case not in best or value < best[case]:
                best[case] = value
    return best


def collect(binary, runs, files):
    cases = {}
    for path in files:
        found = run_case(binary, path, runs)
        if found is None:
            continue
        for case, value in found.items():
            key = f"{os.path.basename(path)}::{case}"
            cases[key] = value
    return cases


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--update", action="store_true", help="re-record the baseline")
    ap.add_argument(
        "--runs",
        type=int,
        default=None,
        help=(
            "runs per case; defaults to the baseline's own value, because the minimum "
            "of N runs is only comparable to another minimum of the same N"
        ),
    )
    ap.add_argument("--tolerance", type=float, default=None, help="percent slower that fails")
    ap.add_argument(
        "--binary",
        default=os.path.join(ROOT, "indent-native", "target", "release", "indent"),
    )
    args = ap.parse_args()

    if not os.access(args.binary, os.X_OK):
        print(f"no binary at {args.binary} — build it first", file=sys.stderr)
        return 2

    files = sorted(
        os.path.join(ROOT, "benchmarks", f)
        for f in os.listdir(os.path.join(ROOT, "benchmarks"))
        if f.startswith("bench_") and f.endswith(".ind")
    )
    if not files:
        print("no benchmarks found", file=sys.stderr)
        return 2

    baseline_doc = {}
    if os.path.exists(BASELINE):
        with open(BASELINE) as fh:
            baseline_doc = json.load(fh)

    # Best-of-N is only comparable to another best-of-N at the same N. Recording with
    # `--runs 5` and checking with `--runs 3` reported ~50% "regressions" on cases worth
    # tens of microseconds, purely because the minimum of three samples is worse than the
    # minimum of five. So the baseline's own count wins unless it is overridden on purpose.
    recorded_runs = baseline_doc.get("runs", 5)
    if args.runs is None:
        runs = recorded_runs
    else:
        runs = args.runs
        if not args.update and runs != recorded_runs:
            print(
                f"note: --runs {runs} overrides the baseline's {recorded_runs}; "
                "expect false regressions",
                file=sys.stderr,
            )

    print(f"running {len(files)} benchmark files x {runs} runs ...")
    measured = collect(args.binary, runs, files)
    if not measured:
        print("no cases parsed — did the benchmark output format change?", file=sys.stderr)
        return 2

    if args.update:
        old = baseline_doc
        version = subprocess.run(
            [args.binary, "--version"], cwd=ROOT, capture_output=True, text=True
        ).stdout.strip()
        doc = {
            "note": (
                "Best-of-N per case, N = `runs` below. Compare with "
                "`python3 benchmarks/check_baselines.py` (which uses the same N); "
                "re-record deliberately with --update."
            ),
            "recorded": str(date.today()),
            "binary": version,
            "runs": runs,
            "tolerance_pct": args.tolerance or old.get("tolerance_pct", 15.0),
            "cases": dict(sorted(measured.items())),
        }
        with open(BASELINE, "w") as fh:
            json.dump(doc, fh, indent=2, sort_keys=False)
            fh.write("\n")
        print(f"recorded {len(measured)} cases to {os.path.relpath(BASELINE, ROOT)}")
        print(f"  from {version}, best of {runs}")
        return 0

    if not baseline_doc:
        print(f"no baseline at {BASELINE} — record one with --update", file=sys.stderr)
        return 2
    doc = baseline_doc
    base = doc.get("cases", {})
    tolerance = args.tolerance if args.tolerance is not None else doc.get("tolerance_pct", 15.0)

    regressed = []
    improved = []
    missing = []
    for case, value in sorted(measured.items()):
        if case not in base:
            missing.append(case)
            continue
        old = base[case]
        if old <= 0:
            continue
        delta = (value - old) / old * 100.0
        if delta > tolerance:
            regressed.append((case, old, value, delta))
        elif delta < -tolerance:
            improved.append((case, old, value, delta))

    print()
    print(f"baseline: {doc.get('binary', '?')} recorded {doc.get('recorded', '?')}")
    print(f"tolerance: {tolerance:.0f}%   cases compared: {len(measured) - len(missing)}")
    print()
    for case, old, value, delta in improved:
        print(f"  faster  {delta:+6.1f}%  {case}   {old:g} -> {value:g}")
    for case, old, value, delta in regressed:
        print(f"  SLOWER  {delta:+6.1f}%  {case}   {old:g} -> {value:g}")
    if missing:
        print(f"  ({len(missing)} case(s) not in the baseline — new benchmark output)")

    if regressed:
        print()
        print(f"FAIL: {len(regressed)} case(s) slower than baseline by more than {tolerance:.0f}%")
        print("  Re-measure before believing it: a loaded machine moves a single run by ~15%.")
        print("  Then either fix it, or record the new cost deliberately with --update.")
        return 1

    print()
    print(f"OK: no case slower than baseline by more than {tolerance:.0f}%")
    return 0


if __name__ == "__main__":
    sys.exit(main())
