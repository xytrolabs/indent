#!/usr/bin/env python3
"""Compare Indent's networking layer with Python's on identical work.

Both languages talk to the same local fixture, doing the same operations: a GET
loop, a POST-JSON loop, parsing a 1000-element JSON body N times, and serialising
a structure N times. Python's side uses `requests` (the closest analogue to the
reqwest-based `http_*` builtins); `urllib.request` is measured too, since it is
the standard library's own client and therefore the fairest "out of the box" bar.

    python3 benchmarks/net_ab_compare.py [--n 50] [--port 8799]

Prints a table of per-iteration costs and the Indent/Python ratio, plus a
one-shot process comparison (start the interpreter, do one GET, exit) — the case
a CLI user actually pays for.
"""
import argparse
import json
import os
import re
import statistics
import subprocess
import sys
import time
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INDENT = os.environ.get("INDENT_BIN", os.path.join(ROOT, "indent-native/target/release/indent"))


def timed(fn, n):
    """Best and median seconds per iteration over n runs."""
    samples = []
    for _ in range(n):
        t0 = time.perf_counter()
        fn()
        samples.append(time.perf_counter() - t0)
    return min(samples), statistics.median(samples)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--n", type=int, default=50)
    ap.add_argument("--port", type=int, default=8799)
    args = ap.parse_args()

    base = f"http://127.0.0.1:{args.port}"
    os.environ["NET_FIXTURE_URL"] = base
    n = args.n

    # The fixture must be up; say so clearly rather than reporting nonsense.
    try:
        urllib.request.urlopen(base + "/health", timeout=3).read()
    except Exception as e:                                     # noqa: BLE001
        print(f"Fixture not reachable at {base} ({e}).")
        print("Start it with: python3 tools/mock_fixture_server.py", args.port)
        return 2

    import requests

    session = requests.Session()          # keep-alive, as reqwest's shared client does
    big_raw = urllib.request.urlopen(base + "/large?n=1000").read()
    body = {"name": "indent", "n": 7, "list": [1, 2, 3, 4, 5], "flag": True}

    # Warm up both stacks.
    session.get(base + "/get")
    session.post(base + "/post", json=body)

    py = {}
    py["get_loop"] = timed(lambda: session.get(base + "/get"), n)
    py["post_loop"] = timed(lambda: session.post(base + "/post", json=body), n)
    py["parse_loop"] = timed(lambda: json.loads(big_raw), n)
    py["dumps_loop"] = timed(lambda: json.dumps(body), n)
    py_urllib_get = timed(lambda: urllib.request.urlopen(base + "/get").read(), n)

    env = dict(os.environ, NET_FIXTURE_URL=base, NET_AB_N=str(n))
    out = subprocess.run(
        [INDENT, "run", os.path.join(ROOT, "benchmarks/net_ab_indent.ind")],
        capture_output=True, text=True, env=env, timeout=600,
    )
    if out.returncode != 0:
        print("Indent side failed:")
        print(out.stdout[-2000:], out.stderr[-2000:])
        return 1

    ind = {}
    for line in out.stdout.splitlines():
        m = re.match(r"RESULT (\w+) ([\d.eE+-]+)", line.strip())
        if m:
            ind[m.group(1)] = float(m.group(2))
    if "n" not in ind:
        print("Could not parse Indent timings:\n" + out.stdout)
        return 1

    def us(seconds, iters):
        return seconds / iters * 1e6

    labels = {
        "get_loop": "GET (keep-alive session)",
        "post_loop": "POST + JSON body",
        "parse_loop": "json parse (1000-item body)",
        "dumps_loop": "json serialise (small dict)",
    }
    print(f"\n{n} iterations each, best-of-run wall time per operation, local fixture")
    print(f"Python {sys.version.split()[0]} (requests {requests.__version__}) vs Indent 2.3\n")
    print(f"{'operation':34} {'Python':>12} {'Indent':>12} {'Indent/Python':>14}")
    print("-" * 76)
    for key, label in labels.items():
        p = us(py[key][0], n)
        i = us(ind[key], n)
        print(f"{label:34} {p:10.1f} µs {i:10.1f} µs {i / p:12.2f}×")
    p = us(py_urllib_get[0], n)
    print(f"{'GET (urllib, new conn each)':34} {p:10.1f} µs {'-':>12} {'-':>14}")

    # ── one-shot process: what a CLI user pays ────────────────────────────
    py_snippet = (
        "import urllib.request,time;"
        "t=time.perf_counter();"
        f"urllib.request.urlopen('{base}/get').read();"
        "print((time.perf_counter()-t)*1e3)"
    )
    ind_snippet = os.path.join("/tmp", "indent_oneshot.ind")
    with open(ind_snippet, "w") as f:
        f.write(
            'var t0 float = time_perf_counter\n'
            f'var r dynamic = http_get "{base}/get"\n'
            'var t1 float = time_perf_counter\n'
            'say string((t1 - t0) * 1000.0)\n'
        )

    def one_shot(cmd, runs=5):
        best = None
        for _ in range(runs):
            t0 = time.perf_counter()
            subprocess.run(cmd, capture_output=True, timeout=120)
            dt = time.perf_counter() - t0
            best = dt if best is None else min(best, dt)
        return best * 1e3

    py_oneshot = one_shot([sys.executable, "-c", py_snippet])
    ind_oneshot = one_shot([INDENT, "run", ind_snippet])
    print()
    print("one-shot: start the interpreter, one GET, exit (best of 5)")
    print(f"  {'python -c':30} {py_oneshot:8.1f} ms")
    print(f"  {'indent run':30} {ind_oneshot:8.1f} ms")
    print(f"  {'ratio':30} {ind_oneshot / py_oneshot:8.2f}×")
    return 0


if __name__ == "__main__":
    sys.exit(main())
