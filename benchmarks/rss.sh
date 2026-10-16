#!/usr/bin/env bash
# Peak RSS of a single run, in KB, and the wall time.
#
# There is no /usr/bin/time on this machine, so the measurement is done from
# Python: `getrusage(RUSAGE_CHILDREN).ru_maxrss` is the high-water mark of the
# child process (and its own children), in KB. Used to check the *memory* claims
# of the COW work, which the ns/op benchmarks cannot see.
#
# Run each candidate several times: the high-water mark is stable, but the first
# run of a freshly written file also pays the AST cache write.
#
#   sh benchmarks/rss.sh benchmarks/bench_string_reads.ind [/path/to/indent] [reps]
set -u

FILE="${1:?usage: rss.sh <file.ind> [binary] [reps]}"
BIN="${2:-indent-native/target/release/indent}"
REPS="${3:-5}"

python3 - "$FILE" "$BIN" "$REPS" <<'PY'
import resource, subprocess, sys, time

path, binary, reps = sys.argv[1], sys.argv[2], int(sys.argv[3])

best = None
for _ in range(reps):
    before = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    t0 = time.perf_counter()
    r = subprocess.run([binary, path], stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL)
    dt = time.perf_counter() - t0
    after = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if r.returncode != 0:
        sys.exit(f"{path} exited {r.returncode}")
    # ru_maxrss is a high-water mark, so a later run can only be >= an earlier
    # one within the same process tree; the first run is the honest reading.
    peak = max(after, before)
    best = peak if best is None else min(best, peak)
    print(f"  run: peak {peak:>8} KB   wall {dt*1000:8.2f} ms")

print(f"{path}: peak RSS {best} KB  ({reps} runs, high-water mark)")
PY
