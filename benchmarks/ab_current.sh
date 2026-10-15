#!/bin/sh
# A/B the current working build against the previously installed release.
# Usage: sh benchmarks/ab_current.sh
OLD=/home/raf/.local/share/indent/bin/indent
NEW=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
OUT=/tmp/ab_current.txt
cd /run/media/raf/Z/Aether.ath || exit 1
: > "$OUT"

best() { # best label binary file
    label="$1"; bin="$2"; file="$3"
    bestv=""
    i=1
    while [ "$i" -le 3 ]; do
        S=$(date +%s%N)
        timeout 120 "$bin" "$file" > /dev/null 2>&1
        E=$(date +%s%N)
        v=$(( (E-S)/1000 ))
        if [ -z "$bestv" ] || [ "$v" -lt "$bestv" ]; then bestv=$v; fi
        i=$((i + 1))
    done
    printf '%-30s %8s us  (best of 3)\n' "$label" "$bestv" >> "$OUT"
}

printf '%s\n' "old = $("$OLD" --version 2>/dev/null)   new = $("$NEW" --version 2>/dev/null)" >> "$OUT"
echo "" >> "$OUT"
for f in benchmarks/bench_loops.ind benchmarks/bench_deep_calls.ind benchmarks/bench_calls.ind benchmarks/bench_hot_paths.ind; do
    echo "=== $f ===" >> "$OUT"
    best "  $f (old)" "$OLD" "$f"
    best "  $f (new)" "$NEW" "$f"
done

echo "" >> "$OUT"
echo "=== startup, 30 runs (per run) ===" >> "$OUT"
for pair in "$OLD:old" "$NEW:new"; do
    bin="${pair%%:*}"; tag="${pair##*:}"
    S=$(date +%s%N); seq 30 | xargs -I@ "$bin" benchmarks/bench_startup_imports.ind > /dev/null 2>&1; E=$(date +%s%N)
    printf '%-30s %8s us\n' "5 std imports ($tag)" "$(( (E-S)/30/1000 ))" >> "$OUT"
done

cat "$OUT"
