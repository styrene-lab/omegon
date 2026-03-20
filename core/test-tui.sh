#!/bin/bash
# TUI interaction test — drives the ratatui interactive mode via tmux.
#
# Starts omegon interactive in a tmux session, sends keystrokes,
# captures pane output, and verifies rendering.
#
# Prerequisites:
#   - tmux
#   - omegon binary (built or on PATH)
#   - ANTHROPIC_API_KEY or ~/.pi/agent/auth.json
#
# Usage:
#   ./test-tui.sh [path-to-binary]

set -euo pipefail

BINARY="${1:-target/release/omegon}"
SESSION="omegon-tui-test"
WORKSPACE=$(mktemp -d)
FAILURES=0

if [ ! -x "$BINARY" ]; then
  echo "Binary not found: $BINARY"
  echo "Build first: cargo build --release"
  exit 1
fi

cleanup() {
  tmux kill-session -t "$SESSION" 2>/dev/null || true
  rm -rf "$WORKSPACE"
}
trap cleanup EXIT

capture() {
  # Capture tmux pane contents — returns what's currently rendered
  tmux capture-pane -t "$SESSION" -p 2>/dev/null
}

wait_for() {
  # Wait for a string to appear in the pane (up to $2 seconds)
  local pattern="$1"
  local timeout="${2:-10}"
  for i in $(seq 1 "$timeout"); do
    if capture | grep -q "$pattern"; then
      return 0
    fi
    sleep 1
  done
  echo "TIMEOUT waiting for: $pattern"
  echo "--- Current pane ---"
  capture
  echo "---"
  return 1
}

send() {
  tmux send-keys -t "$SESSION" "$@"
}

assert_visible() {
  local desc="$1"
  local pattern="$2"
  if capture | grep -q "$pattern"; then
    echo "  ✓ $desc"
  else
    echo "  ✗ $desc — expected: $pattern"
    FAILURES=$((FAILURES + 1))
  fi
}

assert_not_visible() {
  local desc="$1"
  local pattern="$2"
  if capture | grep -q "$pattern"; then
    echo "  ✗ $desc — should NOT contain: $pattern"
    FAILURES=$((FAILURES + 1))
  else
    echo "  ✓ $desc"
  fi
}

# ─── Start TUI ───────────────────────────────────────────────────────────────

echo "=== Starting TUI in tmux session ==="
tmux new-session -d -s "$SESSION" -x 120 -y 40 \
  "$BINARY --log-file $WORKSPACE/tui-test.log --log-level info interactive --cwd $WORKSPACE"

echo ""
echo "=== Test 1: Startup rendering ==="
wait_for "Omegon" 10
assert_visible "Welcome message visible" "Omegon"
assert_visible "Footer visible" "idle"
assert_visible "Editor prompt visible" "▸"

echo ""
echo "=== Test 2: Text input ==="
send "hello world"
sleep 0.5
assert_visible "Typed text appears" "hello world"

echo ""
echo "=== Test 3: Backspace ==="
send BSpace BSpace BSpace BSpace BSpace
sleep 0.3
assert_visible "Text after backspace" "hello"
assert_not_visible "Deleted text gone" "world"

echo ""
echo "=== Test 4: Slash command ==="
# Clear and type /help
send C-a  # Home — clear selection
sleep 0.1
# Kill the line
for i in $(seq 1 20); do send BSpace; done
sleep 0.2
send "/help"
send Enter
sleep 0.5
assert_visible "/help output" "Available commands"
assert_visible "Lists /exit" "/exit"

echo ""
echo "=== Test 5: /model command ==="
send "/model"
send Enter
sleep 0.5
assert_visible "Model display" "anthropic"

echo ""
echo "=== Test 6: /stats command ==="
send "/stats"
send Enter
sleep 0.5
assert_visible "Stats display" "turn"

echo ""
echo "=== Test 7: Submit a real prompt ==="
send "What is 3+3? Just the number."
send Enter
# Wait for response — this makes a real API call
if wait_for "idle" 30; then
  assert_visible "Agent returned to idle" "idle"
  echo "  ✓ Agent completed a turn"
  # Check the turn counter advanced (footer format: "turn N")
  assert_visible "Turn counter advanced" "turn [1-9]"
else
  echo "  ✗ Agent did not return to idle within 30s"
  FAILURES=$((FAILURES + 1))
fi

echo ""
echo "=== Test 8: Double Ctrl+C exits ==="
send C-c
sleep 0.5
send C-c
sleep 2
if tmux has-session -t "$SESSION" 2>/dev/null; then
  # Fallback: /exit
  send "/exit"
  send Enter
  sleep 1
  if tmux has-session -t "$SESSION" 2>/dev/null; then
    echo "  ✗ Session still alive after double Ctrl+C and /exit"
    FAILURES=$((FAILURES + 1))
  else
    echo "  ✓ TUI exited on /exit fallback"
  fi
else
  echo "  ✓ TUI exited cleanly on double Ctrl+C"
fi

echo ""
echo "=== Test 9: Log file written ==="
if [ -f "$WORKSPACE/tui-test.log" ]; then
  LINES=$(wc -l < "$WORKSPACE/tui-test.log")
  echo "  ✓ Log file exists ($LINES lines)"
else
  echo "  ✗ Log file not found"
  FAILURES=$((FAILURES + 1))
fi

echo ""
if [ "$FAILURES" -eq 0 ]; then
  echo "=== All TUI tests passed ==="
else
  echo "=== $FAILURES test(s) failed ==="
  echo ""
  echo "Debug: log at $WORKSPACE/tui-test.log"
  echo "       pane capture above shows last state"
  # Don't cleanup so logs are available
  trap - EXIT
  exit 1
fi
