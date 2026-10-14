#!/bin/sh
# A/B the old (2.2.2) interpreter against the current build on real workloads.
OLD=/home/raf/.local/share/indent/bin/indent-2.2.2.bak
NEW=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
OUT=/tmp/ab_report.txt
cd /run/media/raf/Z/Aether.ath || exit 1
: > "$OUT"

timeit() {
    label="$1"; shift
    S=$(date +%s%N)
    timeout 120 "$@" > /dev/null 2>&1
    E=$(date +%s%N)
    printf '%-34s %8s ms\n' "$label" "$(( (E-S)/1000000 ))" >> "$OUT"
}

echo "=== real programs, single run ===" >> "$OUT"
timeit "game_simulation (2.2.2)" "$OLD" examples/game_simulation.ind
timeit "game_simulation (new)  " "$NEW" examples/game_simulation.ind
timeit "python_interop (2.2.2)" "$OLD" examples/python_interop.ind
timeit "python_interop (new)  " "$NEW" examples/python_interop.ind
timeit "ai_pkg example (2.2.2)" "$OLD" examples/ai_semantic_search.ind
timeit "ai_pkg example (new)  " "$NEW" examples/ai_semantic_search.ind

echo "" >> "$OUT"
echo "=== full test suite (51 files) ===" >> "$OUT"
timeit "indent suite (2.2.2)" "$OLD" test tests
timeit "indent suite (new)  " "$NEW" test tests

echo "" >> "$OUT"
echo "=== std-import startup (30 runs) ===" >> "$OUT"
for pair in "$OLD:2.2.2" "$NEW:new  "; do
    bin="${pair%%:*}"; tag="${pair##*:}"
    S=$(date +%s%N); seq 30 | xargs -I@ "$bin" benchmarks/bench_startup_imports.ind > /dev/null 2>&1; E=$(date +%s%N)
    printf '%-34s %8s us/run\n' "5 std imports ($tag)" "$(( (E-S)/30/1000 ))" >> "$OUT"
    S=$(date +%s%N); seq 30 | xargs -I@ "$bin" /tmp/imp_discord.ind > /dev/null 2>&1; E=$(date +%s%N)
    printf '%-34s %8s us/run\n' "discord import ($tag)" "$(( (E-S)/30/1000 ))" >> "$OUT"
done

echo "" >> "$OUT"
echo "=== function calls into a large body (20k calls) ===" >> "$OUT"
timeit "deep calls (2.2.2)" "$OLD" benchmarks/bench_deep_calls.ind
timeit "deep calls (new)  " "$NEW" benchmarks/bench_deep_calls.ind

cat "$OUT"
