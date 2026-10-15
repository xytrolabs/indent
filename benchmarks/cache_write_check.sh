#!/bin/sh
# Does a run REUSE the cache, or rewrite it? Compare the cache file's mtime and
# size across runs. A rewrite every run would mean the validity check is failing.
IND=/run/media/raf/Z/Aether.ath/indent-native/target/release/indent
OLD=/home/raf/.local/share/indent/bin/indent
D=/tmp/cache_write_test
rm -rf "$D"; mkdir -p "$D"
printf 'get Empty from json\nsay "ok"\n' > "$D/main.ind"

stamp() { # print mtime.ns + size of every .inc under $D
    find "$D" -name '*.inc' -printf '%T@ %s %p\n' 2>/dev/null | sort
}

echo "--- run 1 (new binary) ---"
"$IND" "$D/main.ind" > /dev/null 2>&1
stamp
echo "--- run 2 (new binary), 1s later ---"
sleep 1
"$IND" "$D/main.ind" > /dev/null 2>&1
stamp
echo "--- run 3 (old 2.4.0 binary) ---"
sleep 1
"$OLD" "$D/main.ind" > /dev/null 2>&1
stamp
echo "--- run 4 (new binary again) ---"
sleep 1
"$IND" "$D/main.ind" > /dev/null 2>&1
stamp
