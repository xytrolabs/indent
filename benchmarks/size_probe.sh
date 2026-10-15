#!/bin/sh
# Print the sizes of the interpreter's core types, and enforce the Value
# guardrail (see the `value_size_guardrail` test).
cd /run/media/raf/Z/Aether.ath || exit 1
cargo test --manifest-path indent-native/Cargo.toml --offline value_size_guardrail -- --nocapture 2>&1 | \
    grep -E "SIZEOF|test result"
