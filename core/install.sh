#!/bin/sh
# Install omegon from GitHub Releases.
#
# Usage:
#   curl -fsSL https://omegon.styrene.dev/install.sh | sh
#
# Non-interactive:
#   curl -fsSL https://omegon.styrene.dev/install.sh | sh -s -- --no-confirm
#
# Or directly from GitHub:
#   curl -fsSL https://raw.githubusercontent.com/styrene-lab/omegon/main/install.sh | sh
#
# Environment variables:
#   INSTALL_DIR   — installation directory (default: /usr/local/bin)
#   VERSION       — specific version to install (default: latest)
#   NO_COLOR      — disable colored output (set to any value)
#
# Manual download:
#   https://github.com/styrene-lab/omegon/releases

set -eu

REPO="styrene-lab/omegon"
BINARY="omegon"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
VERSION="${VERSION:-}"
GITHUB_API="https://api.github.com/repos/${REPO}"
TMP=""
NO_CONFIRM=false
RECEIPT_DIR="${HOME}/.config/omegon"

# ── Parse arguments ───────────────────────────────────────────

for arg in "$@"; do
  case "$arg" in
    --no-confirm) NO_CONFIRM=true ;;
    --help|-h)
      echo "Usage: curl -fsSL https://omegon.styrene.dev/install.sh | sh"
      echo ""
      echo "Options (pass after 'sh -s --'):"
      echo "  --no-confirm    Skip interactive confirmation"
      echo ""
      echo "Environment:"
      echo "  INSTALL_DIR     Installation directory (default: /usr/local/bin)"
      echo "  VERSION         Pin a specific version (default: latest)"
      echo "  NO_COLOR        Disable colored output"
      exit 0
      ;;
  esac
done

# ── Color support ─────────────────────────────────────────────

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  ESC=$(printf '\033')
  BOLD="${ESC}[1m"
  DIM="${ESC}[2m"
  CYAN="${ESC}[0;36m"
  GREEN="${ESC}[0;32m"
  YELLOW="${ESC}[0;33m"
  RED="${ESC}[0;31m"
  RESET="${ESC}[0m"
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
  arm64|aarch64) ARCH_NAME="aarch64" ;;
  x86_64|amd64)  ARCH_NAME="x86_64" ;;
  *)
    die "unsupported architecture: $ARCH"
    ;;
esac

# Build Rust target triple to match release asset names
# Assets are: omegon-{VERSION}-{TARGET}.tar.gz
# e.g. omegon-0.15.2-aarch64-apple-darwin.tar.gz
case "$OS_NAME" in
  darwin) TARGET="${ARCH_NAME}-apple-darwin" ;;
  linux)  TARGET="${ARCH_NAME}-unknown-linux-gnu" ;;
esac

PLATFORM="${TARGET}"
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

# Strip 'v' prefix from version for asset names (tags are v0.15.2, assets are omegon-0.15.2-...)
VERSION_NUM=$(echo "$VERSION" | sed 's/^v//')

# Construct the archive name to match release assets
ARCHIVE="${BINARY}-${VERSION_NUM}-${PLATFORM}.tar.gz"

# ── Installation plan ─────────────────────────────────────────

NEEDS_SUDO=false
if [ -d "$INSTALL_DIR" ] && [ ! -w "$INSTALL_DIR" ]; then
  NEEDS_SUDO=true
elif [ ! -d "$INSTALL_DIR" ] && ! mkdir -p "$INSTALL_DIR" 2>/dev/null; then
  NEEDS_SUDO=true
  rmdir "$INSTALL_DIR" 2>/dev/null || true
fi

EXISTING=""
if [ -x "${INSTALL_DIR}/${BINARY}" ]; then
  EXISTING=$("${INSTALL_DIR}/${BINARY}" --version 2>/dev/null | head -1 || true)
  [ -z "$EXISTING" ] && EXISTING="unknown"
fi

printf "  ${BOLD}Installation Plan${RESET}\n"
printf "  ${DIM}────────────────────────────────────────${RESET}\n"
printf "  ${CYAN}Version:${RESET}     %s\n" "${VERSION}"
printf "  ${CYAN}Platform:${RESET}    %s\n" "${PLATFORM}"
printf "  ${CYAN}Install to:${RESET}  ~/.omegon/versions/%s/omegon\n" "${VERSION}"
printf "  ${CYAN}Symlink at:${RESET}  %s\n" "${INSTALL_DIR}/${BINARY}"
if [ -n "$EXISTING" ]; then
  printf "  ${YELLOW}Replaces:${RESET}    %s\n" "${EXISTING}"
fi
if [ "$NEEDS_SUDO" = true ]; then
  printf "  ${YELLOW}Requires:${RESET}    sudo (%s is not writable)\n" "${INSTALL_DIR}"
fi
printf "  ${DIM}Source: github.com/%s${RESET}\n" "${REPO}"
printf "  ${DIM}Integrity: SHA-256 checksum verification${RESET}\n"
echo ""

# ── Confirmation ──────────────────────────────────────────────

if [ "$NO_CONFIRM" = false ] && [ -t 0 ]; then
  printf "  Proceed with installation? ${DIM}[Y/n]${RESET} "
  read -r REPLY < /dev/tty || REPLY="y"
  case "$REPLY" in
    [nN]*) echo "  Cancelled."; exit 0 ;;
  esac
  echo ""
fi

# ── Download ──────────────────────────────────────────────────

BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
ARCHIVE_URL="${BASE_URL}/${ARCHIVE}"
CHECKSUMS_URL="${BASE_URL}/${CHECKSUMS}"

TMP=$(mktemp -d) || die "could not create temporary directory"

step "Downloading ${ARCHIVE}..."

HTTP_CODE=$(curl -fSL -w '%{http_code}' -o "${TMP}/${ARCHIVE}" "$ARCHIVE_URL" 2>/dev/null) || true
if [ ! -f "${TMP}/${ARCHIVE}" ] || [ "$HTTP_CODE" = "404" ]; then
  die "release artifact not found: ${ARCHIVE_URL}

  Available targets: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu
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

  SHORT_HASH=$(printf '%.12s' "$ACTUAL")
  ok "Checksum verified $(dimtext "${SHORT_HASH}…")"
else
  warn "Checksum file not available for this release — skipping verification"
fi

# ── Signature verification (optional — requires cosign) ──────

if command -v cosign >/dev/null 2>&1; then
  SIG_URL="${BASE_URL}/${ARCHIVE}.sig"
  PEM_URL="${BASE_URL}/${ARCHIVE}.pem"
  if curl -fsSL -o "${TMP}/${ARCHIVE}.sig" "$SIG_URL" 2>/dev/null && \
     curl -fsSL -o "${TMP}/${ARCHIVE}.pem" "$PEM_URL" 2>/dev/null; then
    if cosign verify-blob "${TMP}/${ARCHIVE}" \
         --signature "${TMP}/${ARCHIVE}.sig" \
         --certificate "${TMP}/${ARCHIVE}.pem" \
         --certificate-identity-regexp "github.com/styrene-lab/omegon" \
         --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
         >/dev/null 2>&1; then
      ok "Signature verified (Sigstore cosign)"
    else
      warn "Signature verification failed — the binary may not have been built by the official CI"
    fi
  else
    warn "Signature files not available for this release"
  fi
else
  step "Install cosign for cryptographic signature verification"
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

VERSION_DIR="${HOME}/.omegon/versions/${VERSION}"
INSTALL_TARGET="${INSTALL_DIR}/${BINARY}"

step "Installing ${VERSION}..."

# Create versioned install directory
mkdir -p "$VERSION_DIR" || die "could not create version directory ${VERSION_DIR}"

# Handle backward compatibility: move existing flat binary to versioned directory
if [ -f "$INSTALL_TARGET" ] && [ ! -L "$INSTALL_TARGET" ]; then
  step "Migrating existing installation to versioned layout..."
  
  # Determine existing version if possible
  EXISTING_VER=""
  if [ -x "$INSTALL_TARGET" ]; then
    EXISTING_VER=$("$INSTALL_TARGET" --version 2>/dev/null | head -1 | grep -o '[0-9]\+\.[0-9]\+\.[0-9]\+' || echo "unknown")
  fi
  
  if [ "$EXISTING_VER" = "unknown" ]; then
    EXISTING_VER="pre-versioned"
  fi
  
  EXISTING_DIR="${HOME}/.omegon/versions/${EXISTING_VER}"
  mkdir -p "$EXISTING_DIR"
  
  if [ "$NEEDS_SUDO" = true ]; then
    sudo cp "$INSTALL_TARGET" "${EXISTING_DIR}/${BINARY}" || \
      die "could not backup existing binary to ${EXISTING_DIR}"
    sudo chmod +x "${EXISTING_DIR}/${BINARY}"
  else
    cp "$INSTALL_TARGET" "${EXISTING_DIR}/${BINARY}" || \
      die "could not backup existing binary to ${EXISTING_DIR}"
    chmod +x "${EXISTING_DIR}/${BINARY}"
  fi
  
  ok "Backed up existing binary to ${EXISTING_DIR}/${BINARY}"
fi

# Install new version to versioned directory
mv "${TMP}/${BINARY}" "${VERSION_DIR}/${BINARY}"
chmod +x "${VERSION_DIR}/${BINARY}" || die "could not make binary executable"

# Create install directory if needed
if [ ! -d "$INSTALL_DIR" ]; then
  if [ "$NEEDS_SUDO" = true ]; then
    sudo mkdir -p "$INSTALL_DIR" || die "could not create ${INSTALL_DIR}"
  else
    mkdir -p "$INSTALL_DIR"
  fi
fi

# Create or update symlink at install location
if [ "$NEEDS_SUDO" = true ]; then
  # Remove existing binary/symlink
  if [ -e "$INSTALL_TARGET" ] || [ -L "$INSTALL_TARGET" ]; then
    sudo rm -f "$INSTALL_TARGET"
  fi
  
  # Create symlink
  sudo ln -s "${VERSION_DIR}/${BINARY}" "$INSTALL_TARGET" || \
    die "could not create symlink at ${INSTALL_TARGET}"
else
  # Remove existing binary/symlink
  if [ -e "$INSTALL_TARGET" ] || [ -L "$INSTALL_TARGET" ]; then
    rm -f "$INSTALL_TARGET"
  fi
  
  # Create symlink
  ln -s "${VERSION_DIR}/${BINARY}" "$INSTALL_TARGET" || \
    die "could not create symlink at ${INSTALL_TARGET}"
fi

# ── Write install receipt ─────────────────────────────────────

mkdir -p "$RECEIPT_DIR" 2>/dev/null || true
cat > "${RECEIPT_DIR}/install-receipt.json" 2>/dev/null <<EOF || true
{
  "version": "${VERSION}",
  "platform": "${PLATFORM}",
  "install_dir": "${INSTALL_DIR}",
  "binary": "${INSTALL_DIR}/${BINARY}",
  "version_dir": "${VERSION_DIR}",
  "versioned_binary": "${VERSION_DIR}/${BINARY}",
  "installed_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "source": "https://github.com/${REPO}/releases/tag/${VERSION}",
  "installer": "https://omegon.styrene.dev/install.sh",
  "layout": "versioned"
}
EOF

# ── Verify installation ──────────────────────────────────────

INSTALLED_VERSION=""
if command -v "$BINARY" >/dev/null 2>&1; then
  INSTALLED_VERSION=$("${INSTALL_DIR}/${BINARY}" --version 2>/dev/null | head -1 || echo "")
  ok "Installed to ${BOLD}${VERSION_DIR}/${BINARY}${RESET}"
  ok "Symlinked from ${BOLD}${INSTALL_DIR}/${BINARY}${RESET}"
elif [ -x "${INSTALL_DIR}/${BINARY}" ]; then
  warn "${BINARY} installed but ${INSTALL_DIR} is not in your PATH"
  printf "${DIM}    Add it: export PATH=\"${INSTALL_DIR}:\$PATH\"${RESET}\n"
else
  die "installation failed — ${INSTALL_DIR}/${BINARY} is not executable"
fi

# ── Summary ───────────────────────────────────────────────────

echo ""
printf "${BOLD}${GREEN}  ✓ Omegon %s installed successfully${RESET}\n" "${VERSION}"
if [ -n "$INSTALLED_VERSION" ]; then
  printf "${DIM}    %s${RESET}\n" "${INSTALLED_VERSION}"
fi
printf "${DIM}    Receipt: %s/install-receipt.json${RESET}\n" "${RECEIPT_DIR}"
echo ""
printf "  ${BOLD}Quick start${RESET}\n"
printf "  ${DIM}────────────────────────────────────────${RESET}\n"
printf "  ${CYAN}With API key:${RESET}\n"
printf "    ${DIM}export ANTHROPIC_API_KEY=\"sk-ant-...\"${RESET}\n"
printf "    omegon\n"
echo ""
printf "  ${CYAN}With Claude Pro/Max subscription:${RESET}\n"
printf "    omegon login\n"
echo ""
printf "  ${CYAN}One-shot:${RESET}\n"
printf "    omegon --prompt \"hello world\"\n"
echo ""
printf "  ${DIM}Uninstall:${RESET}\n"
printf "    ${DIM}rm %s/%s${RESET}\n" "${INSTALL_DIR}" "${BINARY}"
printf "    ${DIM}rm -rf ~/.omegon/versions${RESET}\n"
printf "    ${DIM}rm -rf ~/.config/omegon${RESET}\n"
echo ""
# Rebuilt 2026-03-25T22:34:14Z
