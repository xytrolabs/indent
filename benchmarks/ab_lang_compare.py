#!/usr/bin/env python3
"""Run benchmarks/ab_lang_bench.ind and benchmarks/ab_lang_bench.py and compare.

Takes the best-of-N result per metric (min, since we want the fastest the
interpreter can go without noise) and prints a table with the Python/Indent
ratio, worst offenders first. This is the tool that decides what to optimize.

    python3 benchmarks/ab_lang_compare.py [reps] [--indent PATH]
"""
import argparse
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
IND = os.path.join(HERE, "ab_lang_bench.ind")
PY = os.path.join(HERE, "ab_lang_bench.py")

# Metrics where a smaller number is not better to report as a slowdown, so we
# can keep the ordering honest.
ORDER = [
    "call_0arg", "call_1arg", "call_2arg",
    "add_assign", "float_mul", "if_compare",
    "str_concat", "int_to_str", "str_to_int",
    "dict_read_hit", "dict_insert", "dict_insert_small",
    "list_append", "list_index_read",
    "list_alloc_small", "dict_alloc_small",
]


def parse(out):
    res = {}
    for line in out.splitlines():
        line = line.strip()
        if not line or line.startswith("checks"):
            continue
        parts = line.split()
        if len(parts) != 2:
            continue
        try:
            res[parts[0]] = float(parts[1])
        except ValueError:
            continue
    return res


def best(cmd, reps, label):
    acc = {}
    err = ""
    for _ in range(reps):
        p = subprocess.run(cmd, capture_output=True, text=True)
        if p.returncode != 0:
            err = p.stderr.strip() or p.stdout.strip()
        for k, v in parse(p.stdout).items():
            if k not in acc or v < acc[k]:
                acc[k] = v
    if not acc:
        print(f"!! {label} produced no measurements")
        if err:
            print(err[:2000])
        sys.exit(1)
    return acc


def find_indent(explicit):
    if explicit:
        return explicit
    for cand in (
        os.path.join(ROOT, "indent-native", "target", "release", "indent"),
        os.path.join(ROOT, "indent-native", "target", "debug", "indent"),
    ):
        if os.path.exists(cand):
            return cand
    from shutil import which
    found = which("indent")
    if found:
        return found
    print("!! no indent binary found; build it or pass --indent PATH")
    sys.exit(1)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("reps", nargs="?", type=int, default=3)
    ap.add_argument("--indent", default=None)
    args = ap.parse_args()

    indent_bin = find_indent(args.indent)
    # --no-cache keeps the AST cache out of the measurement and makes every rep
    # start from the same cold-ish state.
    ind = best([indent_bin, "--no-cache", IND], args.reps, "indent")
    py = best([sys.executable, PY], args.reps, "python")

    keys = [k for k in ORDER if k in ind and k in py]
    keys += [k for k in ind if k in py and k not in keys]

    rows = []
    for k in keys:
        ratio = ind[k] / py[k] if py[k] > 0 else float("inf")
        rows.append((ratio, k, ind[k], py[k]))
    rows.sort(reverse=True)

    print(f"indent: {indent_bin}")
    print(f"python: {sys.executable}")
    print(f"reps:   {args.reps} (best of)")
    print()
    print(f"{'metric':<20}{'indent us':>12}{'python us':>12}{'indent/python':>15}")
    print("-" * 59)
    for ratio, k, iv, pv in rows:
        flag = ""
        if ratio >= 10:
            flag = "  <== worst"
        elif ratio >= 3:
            flag = "  <--"
        print(f"{k:<20}{iv:>12.4f}{pv:>12.4f}{ratio:>14.2f}x{flag}")
    print()
    worst = rows[0]
    print(f"worst: {worst[1]} at {worst[0]:.1f}x "
          f"({worst[2]:.4f} us vs {worst[3]:.4f} us)")
    if len(sys.argv) > 1 and sys.argv[1] == "json":
        import json
        print(json.dumps({k: {"indent": i, "python": p} for _, k, i, p in rows}))


if __name__ == "__main__":
    main()
