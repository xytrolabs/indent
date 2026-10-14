#!/bin/sh
# Rebuild the release binary and report only compile errors.
cd /run/media/raf/Z/Aether.ath || exit 1
cargo build --release --manifest-path indent-native/Cargo.toml 2>&1 | grep -E "^error" -A 10 | head -40
if [ -x indent-native/target/release/indent ]; then
    printf 'BINARY_OK version=%s\n' "$(indent-native/target/release/indent --version)"
else
    printf 'BINARY_MISSING\n'
fi
