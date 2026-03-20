#!/usr/bin/env bash
# Publish omegon npm packages from a GitHub Release.
#
# Downloads release tarballs, extracts binaries into platform package dirs,
# publishes platform packages first, then the wrapper package.
#
# Usage:
#   ./npm/publish.sh                  # uses version from Cargo.toml
#   ./npm/publish.sh 0.13.1           # explicit version
#   ./npm/publish.sh 0.13.1 --dry-run # dry run
#
# Prerequisites:
#   - npm login (or ~/.npmrc with valid token)
#   - gh CLI authenticated
#   - Platform packages must exist on npm (first publish bootstraps them)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO="styrene-lab/omegon-core"

# Parse arguments — supports: publish.sh [VERSION] [--dry-run] in any order
VERSION=""
DRY_RUN=""
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN="--dry-run" ;;
    *)         VERSION="$arg" ;;
  esac
done

if [ -z "$VERSION" ]; then
  VERSION=$(grep -m1 'version = ' "$REPO_ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
fi

TAG="v${VERSION}"
echo "Publishing omegon@${VERSION} from release ${TAG}"
echo ""

# ── Download release assets ──────────────────────────────────────────────
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

PLATFORMS=(darwin-arm64 darwin-x64 linux-arm64 linux-x64)

DOWNLOADED=0

echo "Downloading release assets..."
for platform in "${PLATFORMS[@]}"; do
  asset="omegon-${platform}.tar.gz"
  echo "  ↓ ${asset}"
  gh release download "$TAG" -R "$REPO" -p "$asset" -D "$TMP" 2>/dev/null || {
    echo "  ✗ Failed to download ${asset} — skipping platform"
    continue
  }

  # Extract binary into platform package dir
  platform_dir="$SCRIPT_DIR/platform/$platform"
  tar -xzf "$TMP/$asset" -C "$TMP"
  cp "$TMP/omegon" "$platform_dir/omegon"
  chmod +x "$platform_dir/omegon"
  rm -f "$TMP/omegon"
  echo "  ✓ ${platform}"
  DOWNLOADED=$((DOWNLOADED + 1))
done
echo ""

if [ "$DOWNLOADED" -eq 0 ]; then
  echo "✗ No platform binaries downloaded. Is release ${TAG} published?"
  echo "  Check: gh release view ${TAG} -R ${REPO}"
  exit 1
fi

# ── Update versions ─────────────────────────────────────────────────────
echo "Setting version ${VERSION} across all packages..."
for platform in "${PLATFORMS[@]}"; do
  pkg="$SCRIPT_DIR/platform/$platform/package.json"
  if [ -f "$pkg" ]; then
    # Use node for reliable JSON manipulation
    node -e "
      const fs = require('fs');
      const p = JSON.parse(fs.readFileSync('$pkg', 'utf8'));
      p.version = '$VERSION';
      fs.writeFileSync('$pkg', JSON.stringify(p, null, 2) + '\n');
    "
  fi
done

# Update wrapper package version and optionalDependencies
node -e "
  const fs = require('fs');
  const p = JSON.parse(fs.readFileSync('$SCRIPT_DIR/omegon/package.json', 'utf8'));
  p.version = '$VERSION';
  for (const dep of Object.keys(p.optionalDependencies || {})) {
    p.optionalDependencies[dep] = '$VERSION';
  }
  fs.writeFileSync('$SCRIPT_DIR/omegon/package.json', JSON.stringify(p, null, 2) + '\n');
"
echo ""

# ── Publish platform packages ───────────────────────────────────────────
echo "Publishing platform packages..."
for platform in "${PLATFORMS[@]}"; do
  platform_dir="$SCRIPT_DIR/platform/$platform"
  if [ ! -f "$platform_dir/omegon" ]; then
    echo "  ⊘ @styrene-lab/omegon-${platform} — no binary, skipping"
    continue
  fi

  echo "  ▸ @styrene-lab/omegon-${platform}@${VERSION}"
  (cd "$platform_dir" && npm publish --access public $DRY_RUN 2>&1) || {
    echo "  ✗ Failed to publish @styrene-lab/omegon-${platform}"
    # Don't bail — other platforms may succeed
  }
done
echo ""

# ── Publish wrapper package ─────────────────────────────────────────────
echo "Publishing omegon@${VERSION}..."
(cd "$SCRIPT_DIR/omegon" && npm publish --access public $DRY_RUN 2>&1)
echo ""

# ── Deprecate old TS versions ───────────────────────────────────────────
if [ -z "$DRY_RUN" ]; then
  echo "Deprecating old omegon TS versions (<=0.11.x)..."
  npm deprecate 'omegon@<=0.11.0' \
    'This package now installs the Rust Omegon agent. For the TS TUI: npm i -g omegon-pi' \
    2>/dev/null || true
fi

echo "✓ Done. Verify: npm info omegon@${VERSION}"

# ── Clean up binaries from platform dirs (don't commit them) ────────────
for platform in "${PLATFORMS[@]}"; do
  rm -f "$SCRIPT_DIR/platform/$platform/omegon"
done
