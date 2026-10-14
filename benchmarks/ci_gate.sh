#!/bin/sh
# Verify the release-profile quality gate that CI runs (cargo test --release).
cd /run/media/raf/Z/Aether.ath || exit 1
cargo test --release --manifest-path indent-native/Cargo.toml 2>&1 | \
    grep -E "^error|^warning: .panic|test result|Compiling indent|Finished" | tail -12
