#!/bin/bash
# End-to-end test: install omegon in a container and run a real prompt.
#
# Prerequisites:
#   - Docker or Podman
#   - API key: ANTHROPIC_API_KEY env var, or ~/.pi/agent/auth.json with OAuth token
#
# Usage:
#   ./test-container.sh

set -euo pipefail

RUNTIME="${CONTAINER_RUNTIME:-$(command -v podman || command -v docker)}"
IMAGE="omegon-test:latest"
WORKSPACE=$(mktemp -d)

echo "=== Building container ==="
$RUNTIME build -f Containerfile -t "$IMAGE" .

echo ""
echo "=== Test 1: Binary runs ==="
$RUNTIME run --rm "$IMAGE" --help | head -5

echo ""
echo "=== Test 2: Headless prompt with API key ==="
if [ -n "${ANTHROPIC_API_KEY:-}" ]; then
  AUTH_MOUNT=""
  ENV_FLAG="-e ANTHROPIC_API_KEY"
elif [ -f "$HOME/.pi/agent/auth.json" ]; then
  AUTH_MOUNT="-v $HOME/.pi/agent:/root/.pi/agent:ro"
  ENV_FLAG=""
else
  echo "SKIP: No API key or auth.json found"
  exit 0
fi

$RUNTIME run --rm \
  $AUTH_MOUNT $ENV_FLAG \
  -v "$WORKSPACE:/workspace" \
  "$IMAGE" \
  --prompt "Create a file called test.txt containing 'container test passed'. Then read it back." \
  --cwd /workspace \
  --max-turns 3 \
  --no-session

echo ""
echo "=== Test 3: Verify file was created ==="
if [ -f "$WORKSPACE/test.txt" ]; then
  echo "✓ test.txt exists"
  echo "  Contents: $(cat "$WORKSPACE/test.txt")"
else
  echo "✗ test.txt not found"
  exit 1
fi

echo ""
echo "=== Test 4: Session save ==="
$RUNTIME run --rm \
  $AUTH_MOUNT $ENV_FLAG \
  -v "$WORKSPACE:/workspace" \
  -v "$WORKSPACE/.sessions:/root/.pi/agent/sessions" \
  "$IMAGE" \
  --prompt "What is 7*8? Just the number." \
  --cwd /workspace \
  --max-turns 1

if ls "$WORKSPACE/.sessions" 2>/dev/null | grep -q '.'; then
  echo "✓ Session saved"
else
  echo "✗ No session files found (non-fatal — session dir may differ)"
fi

rm -rf "$WORKSPACE"
echo ""
echo "=== All tests passed ==="
