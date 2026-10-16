#!/usr/bin/env python3
"""Compare released Indent versions on identical workloads: time and peak RSS.

Method, all of which matters for the numbers to mean anything:

- One workload per process, timed start-to-finish. That is what a user pays, and it
  is the only timing that works across releases (the in-language timers changed).
- The child that is *timed* is the child that is *measured*: `fork` + `execv` +
  **blocking** `wait4` gives the wall time and that one child's rusage. Two traps
  avoided: `getrusage(RUSAGE_CHILDREN)` is a high-water mark across every child ever
  reaped, so it reports the same figure for every binary; and reaping through
  `subprocess.communicate` discards the rusage. A first version of this file polled
  with `WNOHANG` + `sleep(0.02)` and every fast number came out a multiple of 20 ms —
  the watchdog's own sleep latency was inside the measurement. The cap is enforced by
  a `threading.Timer` now, so the wait can block and the clock stays honest.
- Each binary runs **in place**, inside its own unpacked release tarball, so it finds
  the `std/` that shipped beside it. Copying the binary out of the tarball makes
  `import` fail, which silently turns a module workload into an error path that every
  version "passes" identically.
- Every workload prints only a checksum, and checksums are compared across versions
  *before* any timing. Different work is not a comparison.
- Repeats are adaptive: a run under `FAST` seconds is repeated up to `RUNS` times and
  the minimum is reported (the minimum ignores machine noise instead of averaging it
  in), while anything slower is measured once. 2.2.2 is *superlinear* on appends and
  copies on every read, so a flat best-of-5 would spend minutes establishing one
  number that the algorithm, not noise, determines.
"""
import os
import signal
import subprocess
import sys
import threading
import time

ROOT = "/run/media/raf/Z/Aether.ath"
WORK = os.path.join(ROOT, "benchmarks", "version-compare")
CACHE = "/tmp/vbench"
OUTDIR = "/tmp/vbench/out"

RUNS = 5
FAST = 5.0    # seconds: slower than this, one run is the measurement
CAP = 90.0    # seconds: slower than this, kill and report a lower bound

RELEASES = {
    "2.2.2": "indent-v2.2.2-x86_64-unknown-linux-gnu",
    "2.3.0": "indent-v2.3.0-x86_64-unknown-linux-gnu",
    "2.4.0": "indent-v2.4.0-x86_64-unknown-linux-gnu",
}
ASSETS = {
    "2.2.2": "v2.2.2/indent-v2.2.2-x86_64-unknown-linux-gnu.tar.gz",
    "2.3.0": "v2.3.0/indent-v2.3.0-x86_64-unknown-linux-gnu.tar.gz",
    "2.4.0": "v2.4.0/indent-v2.4.0-x86_64-unknown-linux-gnu.tar.gz",
}
LOCAL = {"2.4.1-exp1": os.path.join(ROOT, "indent-native", "target", "release", "indent")}

WORKLOADS = [
    "startup", "imports", "calls", "loops", "list_ops", "dict_ops",
    "string_build", "string_read", "list_read", "list_write_alias",
    "memory_strings", "memory_lists", "memory_objects",
]


def fetch(label):
    path = os.path.join(CACHE, RELEASES[label], "indent")
    if os.access(path, os.X_OK):
        return path
    url = f"https://github.com/xytrolabs/indent/releases/download/{ASSETS[label]}"
    tgz = os.path.join(CACHE, f"{label}.tar.gz")
    subprocess.run(["curl", "-fsSL", url, "-o", tgz], check=True)
    subprocess.run(["tar", "xzf", tgz, "-C", CACHE], check=True)
    return path


def binaries():
    out = [(label, fetch(label)) for label in RELEASES] + list(LOCAL.items())
    return [(l, p) for l, p in out if os.access(p, os.X_OK)]


def run_child(binary, program, out_path, cap=CAP):
    """Fork/exec the workload. Returns (exit code, wall s, peak RSS KB, timed out)."""
    start = time.perf_counter()
    pid = os.fork()
    if pid == 0:  # pragma: no cover - child
        fd = os.open(out_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC)
        os.dup2(fd, 1)
        os.dup2(fd, 2)
        os.close(fd)
        os.chdir(ROOT)
        os.execv(binary, [binary, program])
        os._exit(127)

    timed_out = []

    def kill_it():
        timed_out.append(True)
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    watchdog = threading.Timer(cap, kill_it)
    watchdog.start()
    try:
        _, status, ru = os.wait4(pid, 0)  # blocking, so the wall clock is accurate
        wall = time.perf_counter() - start
    finally:
        watchdog.cancel()
    if timed_out:
        return None, cap, None, True
    return os.waitstatus_to_exitcode(status), wall, ru.ru_maxrss, False


def measure(binary, program, runs):
    """(code, last output line, best wall s or None if capped, peak RSS KB)."""
    best = None
    peak = 0
    code = 0
    text = ""
    for i in range(runs):
        out_path = os.path.join(OUTDIR, f"{os.path.basename(program)}.{os.getpid()}.{i}")
        rc, wall, kb, timed_out = run_child(binary, program, out_path)
        with open(out_path, errors="replace") as fh:
            text = fh.read()
        os.unlink(out_path)
        if timed_out:
            return None, "<killed at cap>", None, None
        code = rc
        peak = max(peak, kb)
        if best is None or wall < best:
            best = wall
        if best > FAST:
            break
    return code, text, best, peak


def last_line(text):
    lines = [ln for ln in text.strip().splitlines() if ln.strip()]
    return lines[-1].strip() if lines else "<no output>"


def fmt_time(wall):
    if wall is None:
        return ">90 s"
    if wall >= 1.0:
        return f"{wall:.2f} s"
    return f"{wall * 1000:.1f} ms"


def main():
    os.makedirs(CACHE, exist_ok=True)
    os.makedirs(OUTDIR, exist_ok=True)
    bins = binaries()

    print("=== versions ===", flush=True)
    for label, path in bins:
        v = subprocess.run([path, "--version"], capture_output=True, text=True).stdout.strip()
        print(f"  {label:<10} {v}", flush=True)

    print("\n=== checksums (identical work, or the timing is meaningless) ===", flush=True)
    usable = []
    for w in WORKLOADS:
        program = os.path.join(WORK, f"{w}.ind")
        seen = {}
        for label, path in bins:
            code, text, _, _ = measure(path, program, 1)
            seen[label] = f"rc={code} {last_line(text)}"
        values = set(seen.values())
        if len(values) == 1 and list(values)[0].startswith("rc=0"):
            print(f"  OK       {w:<18} {list(values)[0]}", flush=True)
            usable.append(w)
        else:
            print(f"  EXCLUDED {w}", flush=True)
            for label, value in seen.items():
                print(f"      {label:<10} {value}", flush=True)

    print("\n=== measuring ===", flush=True)
    results = {}
    for w in usable:
        program = os.path.join(WORK, f"{w}.ind")
        results[w] = {}
        for label, path in bins:
            code, text, wall, kb = measure(path, program, RUNS)
            results[w][label] = (wall, kb)
            kb_txt = f"{kb} KB" if kb else "—"
            print(f"  {w:<18} {label:<10} {fmt_time(wall):>9}  {kb_txt:>10}", flush=True)

    labels = [l for l, _ in bins]
    print("\n=== markdown ===", flush=True)
    cols = " | ".join(f"{l} time | {l} RSS" for l in labels)
    print(f"| workload | {cols} |")
    print("|" + "---|" * (1 + 2 * len(labels)))
    for w in usable:
        cells = []
        for label in labels:
            wall, kb = results[w][label]
            cells += [fmt_time(wall), f"{kb} KB" if kb else "—"]
        print(f"| `{w}` | " + " | ".join(cells) + " |")


if __name__ == "__main__":
    sys.exit(main())
