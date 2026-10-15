#!/usr/bin/env bash
# Run the networked parity suites against the local fixture server.
#
#   sh tests/run_net_tests.sh [port]
#
# The fixture (tools/mock_fixture_server.py) serves HTTP, WebSocket and an
# OpenAI-compatible mock, so these suites need no internet, no provider and no API
# key. They are skipped rather than failed by benchmarks/run_suite.sh when the
# fixture is not up, which keeps CI green; this script is the way to run them.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${1:-8799}"
export NET_FIXTURE_URL="http://127.0.0.1:${PORT}"
INDENT="${INDENT_BIN:-$ROOT/indent-native/target/release/indent}"

if [ ! -x "$INDENT" ]; then
    echo "No release binary at $INDENT — run: sh benchmarks/build.sh" >&2
    exit 1
fi

STARTED=0
if curl -sf -m 2 "$NET_FIXTURE_URL/health" >/dev/null 2>&1; then
    echo "Reusing the fixture already listening on $NET_FIXTURE_URL"
else
    python3 "$ROOT/tools/mock_fixture_server.py" "$PORT" >/tmp/indent-fixture.log 2>&1 &
    FIXTURE_PID=$!
    STARTED=1
    for _ in $(seq 1 60); do
        curl -sf -m 1 "$NET_FIXTURE_URL/health" >/dev/null 2>&1 && break
        sleep 0.1
    done
    if ! curl -sf -m 1 "$NET_FIXTURE_URL/health" >/dev/null 2>&1; then
        echo "Fixture failed to start on port $PORT; log:" >&2
        cat /tmp/indent-fixture.log >&2
        exit 1
    fi
    echo "Fixture listening on $NET_FIXTURE_URL"
fi

cleanup() {
    if [ "$STARTED" = "1" ] && [ -n "${FIXTURE_PID:-}" ]; then
        kill "$FIXTURE_PID" 2>/dev/null
        wait "$FIXTURE_PID" 2>/dev/null
    fi
}
trap cleanup EXIT

status=0
for t in net_http_parity net_ws_parity ai_mock_parity; do
    echo
    echo "=== $t ==="
    if ! "$INDENT" run "$ROOT/tests/$t.ind"; then
        status=1
    fi
done

echo
if [ "$status" = "0" ]; then
    echo "Networking + AI parity: all suites passed"
else
    echo "Networking + AI parity: FAILURES (see above)" >&2
fi
exit "$status"
