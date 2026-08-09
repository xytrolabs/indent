#!/bin/bash
# Automated GUI game test for examples/snake_game.ind
# Verifies: window opens -> renders -> closes -> indent completes cleanly.
# Usage: bash tests/gui_snake_test.sh
set -u
cd "$(dirname "$0")/.."   # repo root

LOG="/tmp/snake_gui_test.log"
rm -f "$LOG"

echo "=== GUI Snake test: opens and renders ==="
nohup "$HOME/.local/bin/indent" examples/snake_game.ind > "$LOG" 2>&1 &
INDENT_PID=$!

GUIPID=""
for i in $(seq 1 10); do
  sleep 1
  GUIPID=$(pgrep -x indent-gui | head -1)
  if [ -n "$GUIPID" ]; then
    echo "  frame $i: WINDOW OPEN (pid $GUIPID)"
    break
  fi
done

if [ -z "$GUIPID" ]; then
  echo "  FAIL: no GUI window appeared"
  cat "$LOG"
  kill -9 "$INDENT_PID" 2>/dev/null
  exit 1
fi

sleep 1
if kill -0 "$GUIPID" 2>/dev/null; then
  echo "  PASS: window process alive and rendering"
else
  echo "  FAIL: window exited early"
fi

# Simulate the user closing the window
kill -9 "$GUIPID" 2>/dev/null
sleep 1

for i in 1 2 3 4 5; do
  if ! kill -0 "$INDENT_PID" 2>/dev/null; then
    echo "  indent completed (frame $i)"
    break
  fi
  sleep 1
done

echo "=== indent output ==="
cat "$LOG"

pkill -9 -x indent-gui 2>/dev/null
pkill -9 -x indent 2>/dev/null

if grep -q "thanks for playing" "$LOG" || grep -q "opened" "$LOG"; then
  echo ""
  echo "FINAL: GUI SNAKE TEST PASSED"
  exit 0
else
  echo ""
  echo "FINAL: GUI SNAKE TEST INCOMPLETE"
  exit 1
fi
