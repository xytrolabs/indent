#!/bin/sh
# Run the Indent .ind test suite with a given binary (avoids shell autocorrect
# of the `test` subcommand).
BIN="${1:-/run/media/raf/Z/Aether.ath/indent-native/target/release/indent}"
cd /run/media/raf/Z/Aether.ath || exit 1
"$BIN" test tests
