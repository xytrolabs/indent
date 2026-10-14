#!/bin/sh
# Run the Rust unit tests and print a compact summary (avoids zsh autocorrect
# of the `test` subcommand).
cd /run/media/raf/Z/Aether.ath || exit 1
cargo test --manifest-path indent-native/Cargo.toml --offline 2>&1 | \
    grep -E "^test tests::|^test result|^error|panicked|FAILED" | tail -40
