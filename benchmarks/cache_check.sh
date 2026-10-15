#!/bin/sh
# Exercise the on-disk AST cache (`__incache__/*.inc`) and report concisely.
ROOT=/run/media/raf/Z/Aether.ath
IND="$ROOT/indent-native/target/release/indent"
OUT=/tmp/cache_report.txt
cd "$ROOT" || exit 1
: > "$OUT"

echo "=== 1. clear any existing caches ===" >> "$OUT"
find "$ROOT" -name "__incache__" -not -path "*/target/*" -type d -exec rm -rf {} + 2>/dev/null
find "$ROOT" -name "__incache__" -not -path "*/target/*" | wc -l >> "$OUT"

echo "" >> "$OUT"
echo "=== 2. first run of a script importing discord (cold: parse + write) ===" >> "$OUT"
"$IND" /tmp/imp_discord.ind >> "$OUT" 2>&1

echo "" >> "$OUT"
echo "=== 3. cache files written ===" >> "$OUT"
find "$ROOT" -path "*__incache__*" -name "*.inc" | sed "s|$ROOT/||" >> "$OUT"

echo "" >> "$OUT"
echo "=== 4. startup: 30 runs WITH cache ===" >> "$OUT"
S=$(date +%s%N); seq 30 | xargs -I@ "$IND" /tmp/imp_discord.ind > /dev/null 2>&1; E=$(date +%s%N)
echo "discord import: $(( (E-S)/30/1000 )) us/run" >> "$OUT"

S=$(date +%s%N); seq 30 | xargs -I@ "$IND" benchmarks/bench_startup_imports.ind > /dev/null 2>&1; E=$(date +%s%N)
echo "5 std imports:  $(( (E-S)/30/1000 )) us/run" >> "$OUT"

echo "" >> "$OUT"
echo "=== 5. startup: 30 runs with cache DISABLED ===" >> "$OUT"S=$(date +%s%N); seq 30 | xargs -I@ env INDENT_NO_AST_CACHE=1 "$IND" /tmp/imp_discord.ind > /dev/null 2>&1; E=$(date +%s%N)
echo "discord import: $(( (E-S)/30/1000 )) us/run" >> "$OUT"

S=$(date +%s%N); seq 30 | xargs -I@ env INDENT_NO_AST_CACHE=1 "$IND" benchmarks/bench_startup_imports.ind > /dev/null 2>&1; E=$(date +%s%N)
echo "5 std imports:  $(( (E-S)/30/1000 )) us/run" >> "$OUT"

cat "$OUT"
