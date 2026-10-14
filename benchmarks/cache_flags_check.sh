#!/bin/sh
# Validate the AST-cache CLI flags end to end.
IND=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
D=/tmp/cachetest_flags
rm -rf "$D"; mkdir -p "$D"
printf 'fun Triple\n    give 3\n' > "$D/helper_mod.ind"
printf 'get Triple from helper_mod\nvar t dynamic = Triple()\nsay "ok"\n' > "$D/main.ind"
cd "$D" || exit 1

echo "1. --no-cache run:"
"$IND" --no-cache main.ind
echo "   cache files after --no-cache: $(find . -name '*.inc' | wc -l | tr -d ' ')"

echo "2. normal run:"
"$IND" main.ind
echo "   cache files now: $(find . -name '*.inc' | wc -l | tr -d ' ')"

echo "3. second run reuses the cache:"
"$IND" main.ind

echo "4. edit the module (same length, different value):"
printf 'fun Triple\n    give 4\n' > helper_mod.ind
printf 'get Triple from helper_mod\nvar t dynamic = Triple()\nsay "triple=" + string(t)\n' > main.ind
"$IND" main.ind
echo "   (must print triple=4, not a stale 3)"

echo "5. INDENT_NO_AST_CACHE=1:"
INDENT_NO_AST_CACHE=1 "$IND" main.ind

echo "6. --clear-cache:"
"$IND" --clear-cache
echo "   cache files after clear: $(find . -name '*.inc' | wc -l | tr -d ' ')"
