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
#   NO_COLOR      — disable colored output (set to any value)
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

# ── Color support ─────────────────────────────────────────────

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  BOLD="\033[1m"
  DIM="\033[2m"
  CYAN="\033[0;36m"
  GREEN="\033[0;32m"
  YELLOW="\033[0;33m"
  RED="\033[0;31m"
  RESET="\033[0m"
else
  BOLD="" DIM="" CYAN="" GREEN="" YELLOW="" RED="" RESET=""
fi

# ── Helpers ───────────────────────────────────────────────────

step()    { printf "${CYAN}  ▸${RESET} %s\n" "$*"; }
ok()      { printf "${GREEN}  ✓${RESET} %s\n" "$*"; }
warn()    { printf "${YELLOW}  ⚠${RESET} %s\n" "$*"; }
err()     { printf "${RED}  ✗${RESET} %s\n" "$*" >&2; }
die()     { err "$*"; cleanup; exit 1; }
dimtext() { printf "${DIM}%s${RESET}" "$*"; }

cleanup() {
  if [ -n "$TMP" ] && [ -d "$TMP" ]; then
    rm -rf "$TMP"
  fi
}

# Always clean up, even on error or interrupt
trap cleanup EXIT INT TERM

# ── Preflight checks ─────────────────────────────────────────

command -v curl >/dev/null 2>&1 || die "curl is required but not found"
command -v tar >/dev/null 2>&1 || die "tar is required but not found"

if command -v sha256sum >/dev/null 2>&1; then
  sha256() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  die "sha256sum or shasum is required for checksum verification"
fi

# ── Platform detection ────────────────────────────────────────

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

# ── Banner ────────────────────────────────────────────────────

echo ""
printf "${BOLD}${CYAN}  Ω  Omegon Installer${RESET}\n"
printf "${DIM}  Native AI agent harness — single binary, zero dependencies${RESET}\n"
echo ""

# ── Version resolution ────────────────────────────────────────

if [ -z "$VERSION" ]; then
  step "Resolving latest release..."
  RELEASE_JSON=$(curl -fsSL "${GITHUB_API}/releases/latest" 2>/dev/null) || \
    die "could not reach GitHub API. Check your network connection."

  VERSION=$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

  if [ -z "$VERSION" ]; then
    die "could not determine latest release. Check: https://github.com/${REPO}/releases"
  fi
fi

ok "Version:  ${BOLD}${VERSION}${RESET}"
step "Platform: ${BOLD}${PLATFORM}${RESET}"
echo ""

# ── Download ──────────────────────────────────────────────────

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE_URL="${BASE_URL}/${ARCHIVE}"
CHECKSUMS_URL="${BASE_URL}/${CHECKSUMS}"

TMP=$(mktemp -d) || die "could not create temporary directory"

step "Downloading ${ARCHIVE}..."

HTTP_CODE=$(curl -fSL -w '%{http_code}' -o "${TMP}/${ARCHIVE}" "$ARCHIVE_URL" 2>/dev/null) || true
if [ ! -f "${TMP}/${ARCHIVE}" ] || [ "$HTTP_CODE" = "404" ]; then
  die "release artifact not found: ${ARCHIVE_URL}

  Available platforms: darwin-arm64, darwin-x64, linux-x64, linux-arm64
  Check releases: https://github.com/${REPO}/releases/tag/${VERSION}"
fi

ARCHIVE_SIZE=$(wc -c < "${TMP}/${ARCHIVE}" | tr -d ' ')
if [ "$ARCHIVE_SIZE" -lt 1000 ]; then
  die "downloaded archive is too small (${ARCHIVE_SIZE} bytes) — likely a failed download"
fi

ok "Downloaded $(dimtext "${ARCHIVE_SIZE} bytes")"

# ── Checksum verification ─────────────────────────────────────

step "Verifying checksum..."

if curl -fsSL -o "${TMP}/${CHECKSUMS}" "$CHECKSUMS_URL" 2>/dev/null; then
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

  ok "Checksum verified $(dimtext "${ACTUAL:0:12}…")"
else
  warn "Checksum file not available for this release — skipping verification"
fi

# ── Extract ───────────────────────────────────────────────────

step "Extracting..."

tar xzf "${TMP}/${ARCHIVE}" -C "$TMP" 2>/dev/null || \
  die "failed to extract ${ARCHIVE} — the download may be corrupted"

if [ ! -f "${TMP}/${BINARY}" ]; then
  die "binary '${BINARY}' not found in archive — unexpected archive structure"
fi

# ── Validate binary ───────────────────────────────────────────

FIRST_BYTES=$(head -c 4 "${TMP}/${BINARY}" | xxd -p 2>/dev/null || od -A n -t x1 -N 4 "${TMP}/${BINARY}" | tr -d ' ')

case "$OS_NAME" in
  darwin)
    case "$FIRST_BYTES" in
      feedface*|feedfacf*|cafebabe*|cffaedfe*|cffa*) ;;
      *) die "downloaded file is not a valid macOS binary (magic: ${FIRST_BYTES})" ;;
    esac
    ;;
  linux)
    case "$FIRST_BYTES" in
      7f454c46*) ;;
      *) die "downloaded file is not a valid Linux binary (magic: ${FIRST_BYTES})" ;;
    esac
    ;;
esac

ok "Binary validated"

# ── Install ───────────────────────────────────────────────────

step "Installing to ${INSTALL_DIR}/${BINARY}..."

if [ ! -d "$INSTALL_DIR" ]; then
  if [ -w "$(dirname "$INSTALL_DIR")" ]; then
    mkdir -p "$INSTALL_DIR"
  else
    sudo mkdir -p "$INSTALL_DIR" || die "could not create ${INSTALL_DIR}"
  fi
fi

if [ -w "$INSTALL_DIR" ]; then
  mv "${TMP}/${BINARY}" "${INSTALL_DIR}/${BINARY}"
else
  sudo mv "${TMP}/${BINARY}" "${INSTALL_DIR}/${BINARY}" || \
    die "could not install to ${INSTALL_DIR} — try: INSTALL_DIR=~/.local/bin curl -fsSL … | sh"
fi

chmod +x "${INSTALL_DIR}/${BINARY}" 2>/dev/null || true

# ── Verify installation ──────────────────────────────────────

INSTALLED_VERSION=""
if command -v "$BINARY" >/dev/null 2>&1; then
  INSTALLED_VERSION=$("${INSTALL_DIR}/${BINARY}" --version 2>/dev/null | head -1 || echo "")
  ok "Installed to ${BOLD}${INSTALL_DIR}/${BINARY}${RESET}"
elif [ -x "${INSTALL_DIR}/${BINARY}" ]; then
  warn "${BINARY} installed but ${INSTALL_DIR} is not in your PATH"
  printf "${DIM}    Add it: export PATH=\"${INSTALL_DIR}:\$PATH\"${RESET}\n"
else
  die "installation failed — ${INSTALL_DIR}/${BINARY} is not executable"
fi

# ── Summary ───────────────────────────────────────────────────

echo ""
printf "${BOLD}${GREEN}  ✓ Omegon ${VERSION} installed successfully${RESET}\n"
if [ -n "$INSTALLED_VERSION" ]; then
  printf "${DIM}    ${INSTALLED_VERSION}${RESET}\n"
fi
echo ""
printf "${DIM}  ┌─────────────────────────────────────────────────┐${RESET}\n"
printf "${DIM}  │${RESET}  ${BOLD}Quick start:${RESET}                                    ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}                                                   ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}  ${CYAN}With API key:${RESET}                                  ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}    export ANTHROPIC_API_KEY=\"sk-ant-...\"          ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}    omegon                                         ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}                                                   ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}  ${CYAN}With Claude Pro/Max subscription:${RESET}               ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}    omegon login                                   ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}                                                   ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}  ${CYAN}One-shot prompt:${RESET}                                ${DIM}│${RESET}\n"
printf "${DIM}  │${RESET}    omegon --prompt \"hello world\"                  ${DIM}│${RESET}\n"
printf "${DIM}  └─────────────────────────────────────────────────┘${RESET}\n"
echo ""
