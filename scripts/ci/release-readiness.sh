#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "[1/7] Building and testing runtime"
(
  cd indent-native
  cargo test --release
  cargo build --release
)

echo "[2/7] Verifying installer safety"
./scripts/ci/verify-installer-safety.sh

echo "[3/7] Checking runtime version"
./indent-native/target/release/indent --version

echo "[4/7] Running check on smoke script"
./indent-native/target/release/indent check tests/smoke.ind

echo "[5/7] Running Indent test suite"
./indent-native/target/release/indent test tests

echo "[6/7] Running demo script"
./indent-native/target/release/indent examples/demo.ind

echo "[7/7] Verifying package tooling index access"
CLEAN_HOME="$(mktemp -d)"
trap 'rm -rf "$CLEAN_HOME"' EXIT

HOME="$CLEAN_HOME" ./air index
HOME="$CLEAN_HOME" ./indentpkg index

echo "Release readiness checks passed."
