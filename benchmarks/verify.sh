#!/bin/sh
# Full verification for an Indent release candidate:
#   1. measure performance against the recorded baseline
#   2. run the .ind test suite
#   3. run the Rust unit tests
# Usage: sh benchmarks/verify.sh [output-file]
ROOT=/run/media/raf/Z/Aether.ath
OUT="${1:-/tmp/indent_verify.txt}"
cd "$ROOT" || exit 1
: > "$OUT"

{
  echo "=== performance (release build) ==="
  sh benchmarks/measure.sh ./indent-native/target/release/indent "$OUT.perf"
  cat "$OUT.perf"
  echo
  echo "=== per-operation costs ==="
  ./indent-native/target/release/indent benchmarks/bench_calls.ind
  echo
  echo "=== .ind test suite ==="
  sh benchmarks/run_suite.sh | tail -3
  echo
  echo "=== rust unit tests ==="
  cargo test --manifest-path indent-native/Cargo.toml 2>&1 | grep -E "^test result"
} >> "$OUT" 2>&1

cat "$OUT"
