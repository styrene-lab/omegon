#!/usr/bin/env bash
# Build pi-mono, link omegon globally, and verify the active `pi` binary still
# resolves to Omegon. This is the authoritative dev-mode lifecycle that `/update`
# should match up to the restart handoff boundary.
#
# Usage:
#   ./scripts/install-pi.sh              # build + link + verify
#   ./scripts/install-pi.sh --skip-build # link + verify only (assumes dist/ is current)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PI_MONO="$ROOT_DIR/vendor/pi-mono"

# ── Build ─────────────────────────────────────────────────────────────────
if [[ "${1:-}" != "--skip-build" ]]; then
  echo "▸ Building pi-mono..."
  (cd "$PI_MONO" && npm run build)
else
  echo "▸ Skipping build (--skip-build)"
fi

# ── Refresh deps ──────────────────────────────────────────────────────────
echo "▸ Refreshing omegon dependencies..."
(cd "$ROOT_DIR" && npm install --install-links=false)

# ── Link ──────────────────────────────────────────────────────────────────
echo "▸ Linking omegon globally..."
(cd "$ROOT_DIR" && npm link --force 2>&1 | grep -v "^npm warn")

# ── Verify ────────────────────────────────────────────────────────────────
PI_PATH=$(which pi 2>/dev/null || echo "")
if [[ -z "$PI_PATH" ]]; then
  echo "✗ 'pi' command not found on PATH after linking"
  exit 1
fi

PI_VERSION=$(pi --version 2>/dev/null || echo "FAILED")
PI_REALPATH=$(python3 - <<'PY' "$PI_PATH"
import os, sys
print(os.path.realpath(sys.argv[1]))
PY
)
PI_WHERE=$(pi --where 2>/dev/null || true)

echo ""
echo "✓ pi $PI_VERSION"
echo "  → $PI_PATH"
echo "  ↳ $PI_REALPATH"

if [[ -z "$PI_WHERE" ]]; then
  echo "✗ Active pi binary did not return Omegon runtime metadata"
  exit 1
fi

echo "$PI_WHERE"

if echo "$PI_REALPATH" | grep -q 'omegon' && echo "$PI_WHERE" | grep -q '"omegonRoot"'; then
  echo "✓ Active pi resolves to omegon"
else
  echo "✗ Active pi does not appear to resolve to omegon"
  exit 1
fi

echo ""
echo "✓ Lifecycle complete. Restart pi to pick up the rebuilt runtime."
