#!/bin/sh
# Compare startup between the shipped build and the working build, INTERLEAVED
# and best-of-N, so a drift in machine load cannot favour whichever runs second.
OLD=/home/raf/.local/share/indent/bin/indent
NEW=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
cd /run/media/raf/Z/Aether.ath || exit 1
FILE="${1:-benchmarks/bench_startup_imports.ind}"
RUNS="${2:-5}"

best_old=999999
best_new=999999
i=1
while [ "$i" -le "$RUNS" ]; do
    for pair in "$OLD:o" "$NEW:n"; do
        bin="${pair%%:*}"; tag="${pair##*:}"
        S=$(date +%s%N)
        seq 20 | xargs -I@ "$bin" "$FILE" > /dev/null 2>&1
        E=$(date +%s%N)
        us=$(( (E-S)/20/1000 ))
        if [ "$tag" = "o" ]; then
            [ "$us" -lt "$best_old" ] && best_old=$us
        else
            [ "$us" -lt "$best_new" ] && best_new=$us
        fi
    done
    i=$((i + 1))
done

printf 'startup (%s), best of %s interleaved 20-run batches:\n' "$FILE" "$RUNS"
printf '  installed build : %s us/run\n' "$best_old"
printf '  working build : %s us/run\n' "$best_new"
if [ "$best_new" -lt "$best_old" ]; then
    printf '  -> working build is %s us faster (%.1f%%)\n' "$((best_old - best_new))" \
        "$(awk "BEGIN{print (($best_old-$best_new)/$best_old)*100}")"
else
    printf '  -> working build is %s us slower (%.1f%%)\n' "$((best_new - best_old))" \
        "$(awk "BEGIN{print (($best_new-$best_old)/$best_old)*100}")"
fi
