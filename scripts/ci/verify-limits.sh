#!/usr/bin/env bash
# verify-limits.sh — the execution budgets, end to end.
#
# Indent runs code it did not write (AI output, bot handlers, served routes), and
# two things in it had no ceiling: a `repeat` loop ran until Ctrl-C, and recursion
# ended the *process* with `fatal runtime error: stack overflow`. Both are now
# ordinary Indent errors that the program's host can see and the user can raise.
#
# Every case below therefore checks three things: that the run fails, that it fails
# with the budget's own message, and that it fails *fast* — a budget that only fires
# after the work is done would not be a budget.
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1

BIN=./indent-native/target/release/indent
if [[ ! -x "$BIN" ]]; then
    echo "FAIL: $BIN not built (cd indent-native && cargo build --release)"
    exit 1
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
pass=0
fail=0

ok() { pass=$((pass + 1)); echo "  ok: $1"; }
bad() { fail=$((fail + 1)); echo "  FAIL: $1"; }

# run <label> <expected-substring> <args...>
# The program always runs under `timeout`, so a missing budget fails the case
# instead of hanging this script.
run() {
    local label=$1 expect=$2
    shift 2
    local out rc
    out=$(timeout 20 "$@" 2>&1)
    rc=$?
    if [[ $rc -eq 124 ]]; then
        bad "$label — timed out (the budget did not fire)"
        return
    fi
    if [[ $rc -eq 0 ]]; then
        bad "$label — exited 0, expected a budget error"
        return
    fi
    if [[ "$out" != *"$expect"* ]]; then
        bad "$label — message did not mention '$expect'"
        echo "        got: $(printf '%s' "$out" | tail -2 | tr '\n' ' ')"
        return
    fi
    ok "$label"
}

cat > "$TMP/spin.ind" <<'EOF'
var i = 0
repeat 1000000000
    i += 1
say i
EOF

cat > "$TMP/spin_unbounded.ind" <<'EOF'
var i = 0
repeat
    i += 1
say i
EOF

cat > "$TMP/while_ind.ind" <<'EOF'
var i = 0
repeat while i >= 0
    i += 1
say i
EOF

cat > "$TMP/deep.ind" <<'EOF'
fun down n
    if n <= 0 then give 0
    give down(n - 1) + 1
say down(100000)
EOF

cat > "$TMP/short.ind" <<'EOF'
var total = 0
var i = 0
repeat 10
    total += i
    i += 1
say "short ok " + string(total)
EOF

echo "verify-limits:"

# ---- steps -----------------------------------------------------------------
run "step budget stops a long repeat" "Step limit of 1000" \
    "$BIN" --max-steps 1000 "$TMP/spin.ind"
run "step budget stops an unbounded repeat" "Step limit of 1000" \
    "$BIN" --max-steps 1000 "$TMP/spin_unbounded.ind"
run "step budget stops a compiled while" "Step limit of 1000" \
    "$BIN" --max-steps 1000 "$TMP/while_ind.ind"
run "step budget is enforced with the VM off" "Step limit of 1000" \
    env INDENT_VM=0 "$BIN" --max-steps 1000 "$TMP/spin.ind"
run "step budget via INDENT_MAX_STEPS" "Step limit of 1000" \
    env INDENT_MAX_STEPS=1000 "$BIN" "$TMP/spin.ind"

# ---- wall clock ------------------------------------------------------------
run "time budget stops a long repeat" "Time limit of 150 ms" \
    "$BIN" --timeout-ms 150 "$TMP/spin.ind"

# ---- call depth ------------------------------------------------------------
# This is the case that used to abort the process with a core dump.
run "depth budget stops deep recursion" "Call depth limit of 100" \
    "$BIN" --max-depth 100 "$TMP/deep.ind"
run "depth budget is enforced with the VM off" "Call depth limit of 100" \
    env INDENT_VM=0 "$BIN" --max-depth 100 "$TMP/deep.ind"
run "the default depth budget stops 100k-deep recursion" "Call depth limit of" \
    "$BIN" "$TMP/deep.ind"

# ---- the per-loop ceiling, now raisable ------------------------------------
# It guards loops whose length is *not* written in the source (`while`, `until`,
# a `repeat` with no count). A `repeat N` is already bounded by N, so it is left to
# the step and time budgets instead of being capped a second way.
run "loop ceiling fires on its own" "Loop iteration limit of 50" \
    "$BIN" --max-loop-iters 50 "$TMP/spin_unbounded.ind"
run "loop ceiling fires on a while loop" "Loop iteration limit of 50" \
    "$BIN" --max-loop-iters 50 "$TMP/while_ind.ind"
run "loop ceiling is raisable" "Step limit of 1000" \
    "$BIN" --max-loop-iters 10000000 --max-steps 1000 "$TMP/spin_unbounded.ind"

# ---- budgets must not disturb ordinary programs ----------------------------
out=$("$BIN" --max-steps 1000000 --max-depth 500 --timeout-ms 20000 "$TMP/short.ind" 2>&1)
if [[ "$out" == *"short ok 45"* ]]; then
    ok "a program well inside its budgets is untouched"
else
    bad "a program well inside its budgets is untouched — got: $out"
fi

out=$("$BIN" --max-steps 0 --max-depth 0 --max-loop-iters 0 "$TMP/short.ind" 2>&1)
if [[ "$out" == *"short ok 45"* ]]; then
    ok "0 disables a budget"
else
    bad "0 disables a budget — got: $out"
fi

# A bad value is a usage error, not a silent "no limit".
out=$("$BIN" --max-steps=abc "$TMP/short.ind" 2>&1); rc=$?
if [[ $rc -eq 2 && "$out" == *"Invalid value for --max-steps"* ]]; then
    ok "--max-steps=abc is rejected"
else
    bad "--max-steps=abc is rejected — rc=$rc out=$out"
fi

echo
echo "limits: $pass passed, $fail failed"
[[ $fail -eq 0 ]]
