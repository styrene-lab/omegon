#!/usr/bin/env bash
# record-demo.sh
#
# Generates an asciinema recording of Omegon working in styrene-rs:
#   Scene 1 — Cold start, CIC panel loads
#   Scene 2 — Codebase question (styrene-rns vs styrene-lxmf)
#   Scene 3 — Cleave decomposes a task across 3 crates in parallel
#
# Usage:
#   ./record-demo.sh              # outputs to /tmp/omegon-demo/
#   ./record-demo.sh ./my-output  # custom output dir
#
# Dependencies: asciinema, expect, agg, git, omegon
# Required env: ANTHROPIC_API_KEY

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

REPO_URL="https://github.com/styrene-lab/styrene-rs.git"
OUT_DIR="${1:-/tmp/omegon-demo}"
REPO_DIR="$OUT_DIR/styrene-rs"
CAST_FILE="$OUT_DIR/omegon-demo.cast"
GIF_FILE="$OUT_DIR/omegon-demo.gif"
EXPECT_SCRIPT="$OUT_DIR/driver.exp"

# Terminal dimensions — wide enough for the CIC panel to look good
COLS=220
ROWS=50

# ── Timing (seconds) ──────────────────────────────────────────────────────────
# Tune these if your API responses are faster or slower than the defaults.

T_STARTUP=7          # TUI cold boot before first keypress
T_CIC_BREATHE=3      # Pause after startup so CIC instruments are visible
T_QUESTION=20        # Time for codebase question response
T_CLEAVE=180         # Time for Cleave to run across 3 crates and merge
T_EXIT=3             # After /quit, before recording ends

IDLE_LIMIT=1.5       # Compress idle gaps longer than this in playback

# Typing speed: min and max ms delay between characters (simulates human input)
TYPE_MIN_MS=45
TYPE_MAX_MS=110

# ── Dependency check ──────────────────────────────────────────────────────────

check_deps() {
  local missing=()
  for cmd in asciinema expect agg git omegon; do
    command -v "$cmd" &>/dev/null || missing+=("$cmd")
  done

  if [[ ${#missing[@]} -gt 0 ]]; then
    echo "✗ Missing dependencies: ${missing[*]}" >&2
    echo "  Install via: brew install ${missing[*]}" >&2
    exit 1
  fi

  echo "✓ Dependencies: asciinema $(asciinema --version 2>&1 | head -1 | awk '{print $2}'), expect $(expect -v 2>&1 | awk '{print $3}'), agg $(agg --version 2>&1 | head -1 | awk '{print $2}')"
}

check_env() {
  if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
    echo "✗ ANTHROPIC_API_KEY is not set" >&2
    exit 1
  fi
  echo "✓ ANTHROPIC_API_KEY present (${#ANTHROPIC_API_KEY} chars)"
}

# ── Repo setup ────────────────────────────────────────────────────────────────

clone_repo() {
  echo "→ Cloning styrene-rs to $REPO_DIR..."
  rm -rf "$REPO_DIR"
  git clone --quiet "$REPO_URL" "$REPO_DIR"
  echo "✓ Cloned ($(find "$REPO_DIR/crates/libs" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ') library crates)"
}

# ── Expect driver ─────────────────────────────────────────────────────────────
#
# Drives omegon as if a human were typing. Uses randomised inter-character
# delays to look natural. Bash variables ($T_*) are expanded into the script;
# expect variables (\$foo) are escaped so bash leaves them alone.

write_expect_driver() {
  cat > "$EXPECT_SCRIPT" << EXPECT
#!/usr/bin/expect -f

# type_slowly: sends a string one character at a time with randomised delays
proc type_slowly {text} {
    foreach char [split \$text ""] {
        send -- \$char
        after [expr {$TYPE_MIN_MS + int(rand() * $TYPE_MAX_MS)}]
    }
}

set timeout 300

# ── Scene 1: Cold start ───────────────────────────────────────────────────────
spawn env TERM=xterm-256color omegon

# Wait for TUI to fully boot
sleep $T_STARTUP

# Let the CIC instrument panel breathe before typing
sleep $T_CIC_BREATHE

# ── Scene 2: Codebase question ────────────────────────────────────────────────
type_slowly "what is the relationship between styrene-rns and styrene-lxmf?"
send "\r"

sleep $T_QUESTION

# ── Scene 3: Cleave across 3 crates ──────────────────────────────────────────
type_slowly "add #\[must_use\] to all public Result-returning functions across styrene-rns, styrene-lxmf, and styrene-tunnel"
send "\r"

sleep $T_CLEAVE

# ── Exit cleanly ─────────────────────────────────────────────────────────────
send "/quit\r"

sleep $T_EXIT

EXPECT

  chmod +x "$EXPECT_SCRIPT"
  echo "✓ Expect driver written to $EXPECT_SCRIPT"
}

# ── Recording ─────────────────────────────────────────────────────────────────

record() {
  local estimated=$(( T_STARTUP + T_CIC_BREATHE + T_QUESTION + T_CLEAVE + T_EXIT ))

  echo "→ Recording (~${estimated}s estimated)..."
  echo "  Window: ${COLS}x${ROWS}, idle cap: ${IDLE_LIMIT}s"
  echo "  Output: $CAST_FILE"
  echo ""

  cd "$REPO_DIR"

  asciinema rec "$CAST_FILE" \
    --overwrite \
    --window-size "${COLS}x${ROWS}" \
    --idle-time-limit "$IDLE_LIMIT" \
    --title "Omegon — parallel Cleave execution in styrene-rs" \
    --command "expect $EXPECT_SCRIPT" \
    --quiet

  echo "✓ Cast saved: $CAST_FILE ($(du -sh "$CAST_FILE" | cut -f1))"
}

# ── GIF render ────────────────────────────────────────────────────────────────

render_gif() {
  echo "→ Rendering GIF..."

  agg "$CAST_FILE" "$GIF_FILE" \
    --speed 1.5 \
    --font-size 14

  echo "✓ GIF saved: $GIF_FILE ($(du -sh "$GIF_FILE" | cut -f1))"
}

# ── Summary ───────────────────────────────────────────────────────────────────

print_summary() {
  echo ""
  echo "┌────────────────────────────────────────────────────────────────┐"
  echo "│  Done                                                          │"
  echo "├────────────────────────────────────────────────────────────────┤"
  printf "│  Cast  %-56s │\n" "$CAST_FILE"
  printf "│  GIF   %-56s │\n" "$GIF_FILE"
  echo "├────────────────────────────────────────────────────────────────┤"
  echo "│  Upload to asciinema.com:                                      │"
  printf "│    asciinema upload %-44s │\n" "$CAST_FILE"
  echo "│                                                                │"
  echo "│  Add to README:                                                │"
  echo "│    ![Omegon demo](./demo/omegon-demo.gif)                      │"
  echo "└────────────────────────────────────────────────────────────────┘"
}

# ── Main ──────────────────────────────────────────────────────────────────────

main() {
  echo ""
  echo "  Omegon demo recorder"
  echo "  ─────────────────────────────────────────────"
  echo ""

  mkdir -p "$OUT_DIR"

  check_deps
  check_env
  clone_repo
  write_expect_driver
  record
  render_gif
  print_summary
}

main "$@"
