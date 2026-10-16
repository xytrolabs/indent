#!/bin/sh
# Golden-output check — Phase 0 of the memory plan.
#
# The plan is blunt about why this exists: "you cannot safely rewrite `Value`,
# `Frame`, and the dispatch loop without a way to know you didn't break
# semantics." Assertion-style tests are not that way. They check what their
# author thought of, so two *different* programs can pass them identically —
# while a golden file pins the text a user actually sees.
#
# Every program in `scripts/golden/` is run twice: once through the VM and once
# through the tree-walker (`INDENT_VM=0`). Both have to match the recorded
# `.expected` file byte for byte, and both have to exit 0. That is what makes
# "the compiled path agrees with the interpreter" a check rather than a hope —
# and the compiled path is the one 2.4.0 changed underneath.
#
# Usage:
#   sh scripts/ci/golden-outputs.sh [binary]     # check (default: release build)
#   sh scripts/ci/golden-outputs.sh --update     # re-record the .expected files
#   INDENT_VM_TRACE=1 sh scripts/ci/golden-outputs.sh   # show what the VM compiles
#
# `--update` records the tree-walker's output *only when the VM already agrees
# with it*. Re-recording a divergence would turn a bug into the baseline, which
# is the one way this file could do harm instead of good.
set -u

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GOLDEN="$ROOT/scripts/golden"
BIN=""
UPDATE=0
for arg in "$@"; do
  case "$arg" in
    --update) UPDATE=1 ;;
    *) BIN="$arg" ;;
  esac
done
if [ -z "$BIN" ]; then
  BIN="$ROOT/indent-native/target/release/indent"
fi
# Each fixture runs more than once per engine. The plan asks for exactly this:
# interning or slot assignment can make *ordering* nondeterministic, and a
# single run would never show it. Override with GOLDEN_REPEAT=1 when iterating.
REPEAT=${GOLDEN_REPEAT:-3}
if [ ! -x "$BIN" ]; then
  echo "golden: no binary at $BIN (build it, or pass a path)" >&2
  exit 2
fi

TMP=$(mktemp -d) || exit 2
trap 'rm -rf "$TMP"' EXIT INT TERM

pass=0
fail=0
failed_names=""

# Run one fixture in one engine: writes stdout+stderr, returns the exit status.
run_one() {
    # $1 = fixture path, $2 = output file, $3 = "vm" | "tree"
    if [ "$3" = "tree" ]; then
        ( cd "$ROOT" && INDENT_VM=0 "$BIN" "$1" ) >"$2" 2>&1
    else
        ( cd "$ROOT" && "$BIN" "$1" ) >"$2" 2>&1
    fi
}

for fixture in "$GOLDEN"/*.ind; do
    [ -e "$fixture" ] || continue
    name=$(basename "$fixture" .ind)
    expected="$GOLDEN/$name.expected"

    for engine in vm tree; do
        attempt=1
        while [ "$attempt" -le "$REPEAT" ]; do
        if [ "$REPEAT" -gt 1 ]; then
            out="$TMP/$name.$engine.$attempt"
        else
            out="$TMP/$name.$engine"
        fi
        if run_one "$fixture" "$out" "$engine"; then
            status=0
        else
            status=$?
        fi
        if [ "$status" -ne 0 ]; then
            echo "FAIL $name [$engine]: exited $status"
            sed -n '1,12p' "$out" | sed 's/^/    | /'
            fail=$((fail + 1))
            failed_names="$failed_names $name[$engine]"
            break
        fi

        if [ "$UPDATE" -eq 1 ]; then
            if [ "$engine" = "vm" ]; then
                vm_out="$out"
            else
                # The tree-walker is the reference: if the two engines disagree we
                # record nothing and report it as the failure it is.
                if cmp -s "$vm_out" "$out"; then
                    cp "$out" "$expected"
                    echo "recorded $name.expected"
                    pass=$((pass + 1))
                else
                    echo "FAIL $name: the VM and the tree-walker disagree, so nothing was recorded"
                    diff "$vm_out" "$out" | sed -n '1,20p' | sed 's/^/    | /'
                    fail=$((fail + 1))
                    failed_names="$failed_names $name[engines-differ]"
                fi
            fi
            break
        fi

        if [ ! -f "$expected" ]; then
            echo "FAIL $name [$engine]: no $name.expected (run with --update to record it)"
            fail=$((fail + 1))
            failed_names="$failed_names $name[no-expected]"
            break
        fi

        if cmp -s "$expected" "$out"; then
            if [ "$attempt" -eq "$REPEAT" ]; then
                echo "PASS $name [$engine]${REPEAT}x"
                pass=$((pass + 1))
            fi
        else
            if cmp -s "$TMP/$name.$engine" "$out" 2>/dev/null || [ "$attempt" -eq 1 ]; then
                echo "FAIL $name [$engine]: output differs from $name.expected"
                diff "$expected" "$out" | sed -n '1,20p' | sed 's/^/    | /'
            else
                echo "FAIL $name [$engine]: run $attempt differs from run 1 — the output is not deterministic"
                diff "$TMP/$name.$engine" "$out" | sed -n '1,20p' | sed 's/^/    | /'
            fi
            fail=$((fail + 1))
            failed_names="$failed_names $name[$engine]"
            break
        fi
        attempt=$((attempt + 1))
        done
    done
done

echo
if [ "$UPDATE" -eq 1 ]; then
    echo "Golden outputs: $pass recorded, $fail failed"
else
    echo "Golden outputs: $pass passed, $fail failed"
fi
if [ "$fail" -ne 0 ]; then
    echo "  failing:$failed_names"
    exit 1
fi
