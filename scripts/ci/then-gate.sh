#!/usr/bin/env bash
# Full pre-release gate for the `then` keyword work.
set -uo pipefail
cd /run/media/raf/Z/Aether.ath || exit 1
LOG=/tmp/gate/then_gate.log
mkdir -p /tmp/gate
: > "$LOG"

step() { echo "=== $* ===" | tee -a "$LOG"; }

step "suite (VM on)"
bash benchmarks/run_suite.sh > /tmp/gate/suite_vm1.log 2>&1
echo "rc=$?" | tee -a "$LOG"
grep -E "Test summary|^FAIL" /tmp/gate/suite_vm1.log | tail -3 | tee -a "$LOG"

step "suite (VM off)"
INDENT_VM=0 bash benchmarks/run_suite.sh > /tmp/gate/suite_vm0.log 2>&1
echo "rc=$?" | tee -a "$LOG"
grep -E "Test summary|^FAIL" /tmp/gate/suite_vm0.log | tail -3 | tee -a "$LOG"

step "unit tests"
bash benchmarks/unit_tests.sh > /tmp/gate/units.log 2>&1
echo "rc=$?" | tee -a "$LOG"
tail -3 /tmp/gate/units.log | tee -a "$LOG"

step "golden outputs"
sh scripts/ci/golden-outputs.sh > /tmp/gate/golden.log 2>&1
echo "rc=$?" | tee -a "$LOG"
tail -3 /tmp/gate/golden.log | tee -a "$LOG"

step "installer safety"
bash scripts/ci/verify-installer-safety.sh > /tmp/gate/installer.log 2>&1
echo "rc=$?" | tee -a "$LOG"
tail -3 /tmp/gate/installer.log | tee -a "$LOG"

step "execution budgets"
bash scripts/ci/verify-limits.sh > /tmp/gate/limits.log 2>&1
echo "rc=$?" | tee -a "$LOG"
tail -3 /tmp/gate/limits.log | tee -a "$LOG"

step "baselines"
python3 benchmarks/check_baselines.py > /tmp/gate/baselines.log 2>&1
echo "rc=$?" | tee -a "$LOG"
tail -5 /tmp/gate/baselines.log | tee -a "$LOG"

step "done"
