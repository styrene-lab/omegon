#!/bin/sh
# Install omegon from GitHub Releases.
#
# Usage:
#   curl -fsSL https://omegon.styrene.dev/install.sh | sh
#
# Or directly from GitHub:
#   curl -fsSL https://raw.githubusercontent.com/styrene-lab/omegon-core/main/install.sh | sh
#
# Environment variables:
#   INSTALL_DIR   — installation directory (default: /usr/local/bin)
#   VERSION       — specific version to install (default: latest)
#
# Manual download:
#   https://github.com/styrene-lab/omegon-core/releases

set -eu

REPO="styrene-lab/omegon-core"
BINARY="omegon"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
VERSION="${VERSION:-}"
GITHUB_API="https://api.github.com/repos/${REPO}"
TMP=""

# ── Helpers ───────────────────────────────────────────────────────

log()  { printf '  %s\n' "$*"; }
info() { printf '\033[0;36m%s\033[0m\n' "$*"; }
err()  { printf '\033[0;31mError: %s\033[0m\n' "$*" >&2; }
die()  { err "$*"; cleanup; exit 1; }

cleanup() {
  if [ -n "$TMP" ] && [ -d "$TMP" ]; then
    rm -rf "$TMP"
  fi
}

# Always clean up, even on error or interrupt
trap cleanup EXIT INT TERM

# ── Preflight checks ─────────────────────────────────────────────

# Require curl
command -v curl >/dev/null 2>&1 || die "curl is required but not found"

# Require tar
command -v tar >/dev/null 2>&1 || die "tar is required but not found"

# Require a checksum tool
if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "sha256sum or shasum is required for checksum verification"
fi

# ── Platform detection ────────────────────────────────────────────

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
  darwin) OS_NAME="darwin" ;;
  linux)  OS_NAME="linux" ;;
  *)
    die "unsupported OS: $OS (omegon supports macOS and Linux; Windows users: use WSL)"
    ;;
esac

case "$ARCH" in
  arm64|aarch64) ARCH_NAME="arm64" ;;
  x86_64|amd64)  ARCH_NAME="x64" ;;
  *)
    die "unsupported architecture: $ARCH"
    ;;
esac

PLATFORM="${OS_NAME}-${ARCH_NAME}"
ARCHIVE="${BINARY}-${PLATFORM}.tar.gz"
CHECKSUMS="checksums.sha256"

# ── Version resolution ────────────────────────────────────────────

info "Ω omegon installer"
echo ""

if [ -z "$VERSION" ]; then
  log "Detecting latest release..."
  # Use GitHub API to get latest release tag
  RELEASE_JSON=$(curl -fsSL "${GITHUB_API}/releases/latest" 2>/dev/null) || \
    die "could not reach GitHub API. Check your network connection."

  VERSION=$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

  if [ -z "$VERSION" ]; then
    die "could not determine latest release. Check: https://github.com/${REPO}/releases"
  fi
fi

log "Version:  ${VERSION}"
log "Platform: ${PLATFORM}"
echo ""

# ── Download ──────────────────────────────────────────────────────

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE_URL="${BASE_URL}/${ARCHIVE}"
CHECKSUMS_URL="${BASE_URL}/${CHECKSUMS}"

TMP=$(mktemp -d) || die "could not create temporary directory"

log "Downloading ${ARCHIVE}..."

# Download the archive
HTTP_CODE=$(curl -fSL -w '%{http_code}' -o "${TMP}/${ARCHIVE}" "$ARCHIVE_URL" 2>/dev/null) || true
if [ ! -f "${TMP}/${ARCHIVE}" ] || [ "$HTTP_CODE" = "404" ]; then
  die "release artifact not found: ${ARCHIVE_URL}
  
  Available platforms: darwin-arm64, darwin-x64, linux-x64, linux-arm64
  Check releases: https://github.com/${REPO}/releases/tag/${VERSION}"
fi

# Verify the download is not empty / truncated
ARCHIVE_SIZE=$(wc -c < "${TMP}/${ARCHIVE}" | tr -d ' ')
if [ "$ARCHIVE_SIZE" -lt 1000 ]; then
  die "downloaded archive is too small (${ARCHIVE_SIZE} bytes) — likely a failed download"
fi

# ── Checksum verification ─────────────────────────────────────────

log "Verifying checksum..."

if curl -fsSL -o "${TMP}/${CHECKSUMS}" "$CHECKSUMS_URL" 2>/dev/null; then
  # Extract expected checksum for our archive
  EXPECTED=$(grep "${ARCHIVE}" "${TMP}/${CHECKSUMS}" | cut -d' ' -f1)

  if [ -z "$EXPECTED" ]; then
    die "checksum for ${ARCHIVE} not found in ${CHECKSUMS}"
  fi

  ACTUAL=$(sha256 "${TMP}/${ARCHIVE}")

  if [ "$EXPECTED" != "$ACTUAL" ]; then
    die "checksum mismatch!
    Expected: ${EXPECTED}
    Actual:   ${ACTUAL}
    
    The download may be corrupted or tampered with.
    Try again, or download manually from:
      https://github.com/${REPO}/releases/tag/${VERSION}"
  fi

  log "Checksum OK: ${ACTUAL}"
else
  # Checksums file not available (older releases before we added it)
  log "⚠ Checksum file not available for this release — skipping verification"
fi

# ── Extract ───────────────────────────────────────────────────────

log "Extracting..."

tar xzf "${TMP}/${ARCHIVE}" -C "$TMP" 2>/dev/null || \
  die "failed to extract ${ARCHIVE} — the download may be corrupted"

# Verify the binary exists and is executable
if [ ! -f "${TMP}/${BINARY}" ]; then
  die "binary '${BINARY}' not found in archive — unexpected archive structure"
fi

# ── Validate binary ───────────────────────────────────────────────

# Quick sanity check: the binary should be an executable, not a text file or HTML error page
FIRST_BYTES=$(head -c 4 "${TMP}/${BINARY}" | xxd -p 2>/dev/null || od -A n -t x1 -N 4 "${TMP}/${BINARY}" | tr -d ' ')

case "$OS_NAME" in
  darwin)
    # Mach-O magic: feedface (32-bit) or feedfacf (64-bit) or cafebabe (fat/universal)
    case "$FIRST_BYTES" in
      feedface*|feedfacf*|cafebabe*|cffaedfe*|cffa*) ;; # valid Mach-O
      *) die "downloaded file is not a valid macOS binary (magic: ${FIRST_BYTES})" ;;
    esac
    ;;
  linux)
    # ELF magic: 7f454c46
    case "$FIRST_BYTES" in
      7f454c46*) ;; # valid ELF
      *) die "downloaded file is not a valid Linux binary (magic: ${FIRST_BYTES})" ;;
    esac
    ;;
esac

# ── Install ───────────────────────────────────────────────────────

log "Installing to ${INSTALL_DIR}/${BINARY}..."

# Create install directory if it doesn't exist
if [ ! -d "$INSTALL_DIR" ]; then
  if [ -w "$(dirname "$INSTALL_DIR")" ]; then
    mkdir -p "$INSTALL_DIR"
  else
    sudo mkdir -p "$INSTALL_DIR" || die "could not create ${INSTALL_DIR}"
  fi
fi

# Move binary into place
if [ -w "$INSTALL_DIR" ]; then
  mv "${TMP}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
else
  sudo mv "${TMP}/${BINARY}" "${INSTALL_DIR}/${BINARY}" || \
    die "could not install to ${INSTALL_DIR} — try: INSTALL_DIR=~/.local/bin sh install.sh"
fi

chmod +x "${INSTALL_DIR}/${BINARY}" 2>/dev/null || true

# ── Verify installation ──────────────────────────────────────────

if ! command -v "$BINARY" >/dev/null 2>&1; then
  if [ -x "${INSTALL_DIR}/${BINARY}" ]; then
    log "⚠ ${BINARY} installed but ${INSTALL_DIR} is not in your PATH"
    log "  Add it:  export PATH=\"${INSTALL_DIR}:\$PATH\""
  else
    die "installation failed — ${INSTALL_DIR}/${BINARY} is not executable"
  fi
fi

# ── Done ──────────────────────────────────────────────────────────

echo ""
info "✓ Installed ${BINARY} ${VERSION} to ${INSTALL_DIR}/${BINARY}"
echo ""
log "Get started:"
log ""
log "  # With API key:"
log "  export ANTHROPIC_API_KEY=\"sk-ant-...\""
log "  ${BINARY} --prompt \"hello world\""
log ""
log "  # With subscription (Claude Pro/Max):"
log "  ${BINARY} login"
log ""
log "  # Interactive mode:"
log "  ${BINARY} interactive"
echo ""
