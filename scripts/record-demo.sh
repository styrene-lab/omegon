#!/usr/bin/env bash
# record-demo.sh
#
# Generates an asciinema recording of Omegon working in styrene-rs.
# Drives entirely via --prompt-file — no interactive input required.
#
# Usage:
#   ./record-demo.sh              # outputs to /tmp/omegon-demo/
#   ./record-demo.sh ./my-output  # custom output dir
#
# Dependencies: asciinema, agg, git, omegon
# Required env: ANTHROPIC_API_KEY

set -euo pipefail

# ── Configuration ─────────────────────────────────────────────────────────────

REPO_URL="https://github.com/styrene-lab/styrene-rs.git"
OUT_DIR="${1:-/tmp/omegon-demo}"
REPO_DIR="$OUT_DIR/styrene-rs"
CAST_FILE="$OUT_DIR/omegon-demo.cast"
GIF_FILE="$OUT_DIR/omegon-demo.gif"
PROMPT_FILE="$(cd "$(dirname "$0")" && pwd)/styrene-demo.md"

COLS=220
ROWS=50
IDLE_LIMIT=1.5

# ── Checks ────────────────────────────────────────────────────────────────────

for cmd in asciinema agg git omegon; do
  command -v "$cmd" &>/dev/null || { echo "✗ Missing: $cmd" >&2; exit 1; }
done

[[ -n "${ANTHROPIC_API_KEY:-}" ]] || { echo "✗ ANTHROPIC_API_KEY not set" >&2; exit 1; }
[[ -f "$PROMPT_FILE" ]] || { echo "✗ Prompt file not found: $PROMPT_FILE" >&2; exit 1; }

echo "✓ omegon $(omegon --version 2>&1 | head -1)"
echo "✓ asciinema $(asciinema --version 2>&1 | head -1)"

# ── Clone ─────────────────────────────────────────────────────────────────────

mkdir -p "$OUT_DIR"
echo "→ Cloning styrene-rs..."
rm -rf "$REPO_DIR"
git clone --quiet "$REPO_URL" "$REPO_DIR"
echo "✓ Cloned ($(find "$REPO_DIR/crates/libs" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ') library crates)"

# ── Record ────────────────────────────────────────────────────────────────────

echo "→ Recording..."

asciinema rec "$CAST_FILE" \
  --overwrite \
  --window-size "${COLS}x${ROWS}" \
  --idle-time-limit "$IDLE_LIMIT" \
  --title "Omegon — parallel Cleave execution in styrene-rs" \
  --command "omegon --prompt-file $PROMPT_FILE --cwd $REPO_DIR"

echo "✓ Cast: $CAST_FILE ($(du -sh "$CAST_FILE" | cut -f1))"

# ── Render GIF ────────────────────────────────────────────────────────────────

echo "→ Rendering GIF..."
agg "$CAST_FILE" "$GIF_FILE" --speed 1.5 --font-size 14
echo "✓ GIF:  $GIF_FILE ($(du -sh "$GIF_FILE" | cut -f1))"

# ── Done ──────────────────────────────────────────────────────────────────────

echo ""
echo "Upload:   asciinema upload $CAST_FILE"
echo "README:   ![Omegon demo](./demo/omegon-demo.gif)"
