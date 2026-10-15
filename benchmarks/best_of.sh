#!/bin/sh
# Run a benchmark N times and report the BEST time per case — the minimum is the
# most stable estimator for CPU-bound micro-benchmarks (the machine is noisy).
# Usage: sh benchmarks/best_of.sh <file.ind> [runs] [binary]
IND="${3:-/run/media/raf/Z/Aether.ath/indent-native/target/release/indent}"
FILE="$1"
RUNS="${2:-3}"
OUT=/tmp/best_of.txt
cd /run/media/raf/Z/Aether.ath || exit 1
: > "$OUT"
rm -f "$OUT.raw"

i=1
while [ "$i" -le "$RUNS" ]; do
    "$IND" "$FILE" 2>&1 | grep -E "us/op|us/call|: *[0-9.]+s" >> "$OUT.raw"
    echo "---" >> "$OUT.raw"
    i=$((i + 1))
done

# Keep the lowest numeric value seen for each label.
awk -F: '
{
    key = $1
    val = $2
    gsub(/[^0-9.]/, "", val)
    if (val == "") next
    if (!(key in best) || val + 0 < best[key] + 0) best[key] = val + 0
    if (!(key in count)) { order[++n] = key }
    count[key]++
}
END {
    for (i = 1; i <= n; i++) {
        k = order[i]
        printf "%-24s %s  (best of %d)\n", k, best[k], count[k]
    }
}' "$OUT.raw"
rm -f "$OUT.raw"
