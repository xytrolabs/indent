#!/bin/sh
# Run the Indent .ind test suite with a given binary (avoids shell autocorrect
# of the `test` subcommand).
#
# The runner is in-process, so a test file that calls `process_exit` ends the
# whole run — and `tests/ai_mock_parity.ind` does exactly that when its fixture
# server is not up. The output then stops at the SKIP line, the summary line is
# never printed, and the exit status is still 0: a run that executed one test out
# of seventy-five was indistinguishable from a clean pass. So the summary line is
# the contract now — no summary, no pass — and the message says what to start.
#
# To include the mock-backed tests:  python3 tools/mock_fixture_server.py 8799 &
BIN="${1:-/run/media/raf/Z/Aether.ath/indent-native/target/release/indent}"
cd /run/media/raf/Z/Aether.ath || exit 1

# Start the mock fixture the ai tests need, so the gate is self-contained. If one
# is already listening the bind fails harmlessly and the running one is used; the
# suite's own health check decides what to do either way.
FIXTURE_PID=""
if command -v python3 >/dev/null 2>&1 && [ -f tools/mock_fixture_server.py ]; then
    python3 tools/mock_fixture_server.py 8799 > /tmp/indent_fixture.log 2>&1 &
    FIXTURE_PID=$!
    sleep 1
fi

OUT=$("$BIN" test tests 2>&1)
RC=$?
printf '%s\n' "$OUT"

if [ -n "$FIXTURE_PID" ]; then
    kill "$FIXTURE_PID" 2>/dev/null
    wait "$FIXTURE_PID" 2>/dev/null
fi

if ! printf '%s\n' "$OUT" | grep -q '^Test summary:'; then
    echo "" >&2
    echo "run_suite: the suite stopped before its summary line." >&2
    echo "  A test that calls process_exit ends the whole in-process run —" >&2
    echo "  tests/ai_mock_parity.ind does this when its fixture is missing." >&2
    echo "  Start it by hand with: python3 tools/mock_fixture_server.py 8799 &" >&2
    exit 1
fi

exit "$RC"
