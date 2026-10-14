#!/bin/sh
# Timed Indent performance probe. Usage: sh benchmarks/measure.sh <indent-binary>
IND="${1:-./indent-native/target/release/indent}"
OUT="${2:-/tmp/indent_perf.txt}"

: > "$OUT"

ms_per_run() {
    label="$1"; n="$2"; shift 2
    S=$(date +%s%N)
    seq "$n" | xargs -I@ "$@" > /dev/null 2>&1
    E=$(date +%s%N)
    echo "$label: $(( (E-S)/1000000 ))ms total over $n runs = $(( (E-S)/n/1000000 ))ms/run" >> "$OUT"
}

ms_per_run "startup: hello-world file" 50 "$IND" benchmarks/bench_startup_empty.ind
ms_per_run "startup: import std json" 30 "$IND" benchmarks/bench_startup_imports.ind
ms_per_run "script: 100k calls" 5 "$IND" benchmarks/bench_hot_paths.ind

echo "--- done ---" >> "$OUT"
