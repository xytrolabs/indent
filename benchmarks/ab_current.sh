#!/bin/sh
# A/B the current working build against the previously installed release.
# Usage: sh benchmarks/ab_current.sh
OLD=/home/raf/.local/share/indent/bin/indent
NEW=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
OUT=/tmp/ab_current.txt
cd /run/media/raf/Z/Aether.ath || exit 1
: > "$OUT"

ab_pair() { # ab_pair label file reps — INTERLEAVED old/new, best of N each.
    # Interleaved on purpose: run all of old and then all of new and whichever
    # goes second inherits the first one's turbo/thermal state AND its cache
    # state. The two builds share an AST cache (it is keyed by version, and a
    # dev build of 2.3.0 has the same version string as the installed 2.3.0),
    # so the second binary silently reads the cache the first one wrote. That
    # is how the startup section used to report 59% when the truth was ~2%.
    # $file is deliberately unquoted so a caller can pass flags, e.g.
    # "--no-cache benchmarks/bench_startup_imports.ind".
    label="$1"; file="$2"; reps="$3"
    ob=""; nb=""; i=1
    while [ "$i" -le "$reps" ]; do
        S=$(date +%s%N); timeout 120 "$OLD" $file > /dev/null 2>&1; E=$(date +%s%N)
        v=$(( (E-S)/1000 )); if [ -z "$ob" ] || [ "$v" -lt "$ob" ]; then ob=$v; fi
        S=$(date +%s%N); timeout 120 "$NEW" $file > /dev/null 2>&1; E=$(date +%s%N)
        v=$(( (E-S)/1000 )); if [ -z "$nb" ] || [ "$v" -lt "$nb" ]; then nb=$v; fi
        i=$((i + 1))
    done
    printf '  %-26s %8s us  (best of %s)\n' "$label (old)" "$ob" "$reps" >> "$OUT"
    printf '  %-26s %8s us  (best of %s)\n' "$label (new)" "$nb" "$reps" >> "$OUT"
    printf '  %-26s %8s%%\n' "$label (new vs old)" "$(( (ob-nb)*100/ob ))" >> "$OUT"
}

printf '%s\n' "old = $("$OLD" --version 2>/dev/null)   new = $("$NEW" --version 2>/dev/null)" >> "$OUT"
echo "" >> "$OUT"
for f in benchmarks/bench_loops.ind benchmarks/bench_deep_calls.ind benchmarks/bench_calls.ind benchmarks/bench_hot_paths.ind; do
    echo "=== $f ===" >> "$OUT"
    ab_pair "$f" "$f" 3
done

echo "" >> "$OUT"
echo "=== startup, interleaved, --no-cache (identical cache state by construction) ===" >> "$OUT"
ab_pair "5 std imports" "--no-cache benchmarks/bench_startup_imports.ind" 30

cat "$OUT"
