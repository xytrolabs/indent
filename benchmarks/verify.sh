#!/bin/sh
# Full verification for an Indent release candidate:
#   1. compare performance against the recorded baseline (benchmarks/baselines.json)
#   2. golden outputs: every program in scripts/golden/ through both engines
#   3. run the .ind test suite, in both engines
#   4. run the Rust unit tests
#   5. the installer safety checks
# Usage: sh benchmarks/verify.sh [output-file]
ROOT=/run/media/raf/Z/Aether.ath
OUT="${1:-/tmp/indent_verify.txt}"
cd "$ROOT" || exit 1
: > "$OUT"

{
  echo "=== performance against the recorded baseline ==="
  python3 benchmarks/check_baselines.py | tail -8
  echo
  echo "=== raw performance numbers ==="
  sh benchmarks/measure.sh ./indent-native/target/release/indent "$OUT.perf"
  cat "$OUT.perf"
  echo
  echo "=== per-operation costs ==="
  ./indent-native/target/release/indent benchmarks/bench_calls.ind
  echo
  echo "=== golden outputs (VM and tree-walker, 3 runs each) ==="
  sh scripts/ci/golden-outputs.sh | tail -4
  echo
  echo "=== .ind test suite (VM) ==="
  sh benchmarks/run_suite.sh | tail -3
  echo
  echo "=== .ind test suite (INDENT_VM=0) ==="
  INDENT_VM=0 sh benchmarks/run_suite.sh | tail -3
  echo
  echo "=== rust unit tests ==="
  cargo test --manifest-path indent-native/Cargo.toml 2>&1 | grep -E "^test result"
  echo
  echo "=== installer safety ==="
  sh scripts/ci/verify-installer-safety.sh 2>&1 | tail -2
} >> "$OUT" 2>&1

cat "$OUT"
