#!/usr/bin/env bash
# shellcheck shell=bash disable=SC2059,SC2016,SC1091
# =============================================================================
# pi-kit bootstrap.sh
#
# Detects the host OS, checks for all pi-kit dependencies, installs any that
# are missing, and runs validation checks at the end.
#
# Supported platforms:
#   - macOS (Homebrew)
#   - Debian / Ubuntu (apt)
#   - Fedora / RHEL / CentOS (dnf)
#   - Arch Linux (pacman)
#
# Usage:
#   chmod +x bootstrap.sh && ./bootstrap.sh
#   ./bootstrap.sh --yes    # Non-interactive mode (auto-accept all installs)
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Non-interactive mode
# ---------------------------------------------------------------------------
AUTO_YES=false
[[ "${1:-}" =~ ^(-y|--yes)$ ]] && AUTO_YES=true

# ---------------------------------------------------------------------------
# Colors & helpers
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

info()    { printf "${BLUE}ℹ${RESET}  %s\n" "$*"; }
success() { printf "${GREEN}✅${RESET} %s\n" "$*"; }
warn()    { printf "${YELLOW}⚠${RESET}  %s\n" "$*"; }
fail()    { printf "${RED}❌${RESET} %s\n" "$*"; }
header()  { printf "\n${BOLD}${CYAN}━━━ %s ━━━${RESET}\n\n" "$*"; }

# Track validation results
PASS=0
FAIL=0
WARN=0

check_pass() { success "$1"; (( PASS++ )) || true; }
check_fail() { fail "$1";    (( FAIL++ )) || true; }
check_warn() { warn "$1";    (( WARN++ )) || true; }

has_cmd() { command -v "$1" &>/dev/null; }

# Prompt helper — auto-accepts in --yes mode
confirm() {
    local prompt="$1"
    if [ "$AUTO_YES" = true ]; then
        printf "   %s [y/N] y (--yes)\n" "$prompt"
        return 0
    fi
    read -rp "   ${prompt} [y/N] " yn
    [[ "${yn}" =~ ^[Yy] ]]
}

# ---------------------------------------------------------------------------
# Ensure ~/.local/bin is on PATH (used by uv, pipx, etc.)
# ---------------------------------------------------------------------------
ensure_local_bin_on_path() {
    local shell_name shell_rc

    shell_name="$(basename "$SHELL")"
    case "$shell_name" in
        zsh)  shell_rc="$HOME/.zshrc" ;;
        bash) shell_rc="$HOME/.bashrc" ;;
        fish) shell_rc="$HOME/.config/fish/config.fish" ;;
        *)    shell_rc="$HOME/.profile" ;;
    esac

    # Already in profile — nothing to do
    if grep -q '.local/bin' "$shell_rc" 2>/dev/null; then
        return 0
    fi

    warn "\$HOME/.local/bin is not in your shell profile"
    if confirm "Add it to ${shell_rc}?"; then
        if [ "$shell_name" = "fish" ]; then
            echo 'fish_add_path ~/.local/bin' >> "$shell_rc"
        else
            {
                echo ''
                echo '# Added by pi-kit bootstrap — uv (Python project manager)'
                echo 'export PATH="$HOME/.local/bin:$PATH"'
            } >> "$shell_rc"
        fi
        success "Added ~/.local/bin to ${shell_rc}"
        info "Run: source ${shell_rc}  (or open a new terminal)"
    else
        warn "uv works now, but won't be found in new terminals until ~/.local/bin is on PATH"
    fi
}

# ---------------------------------------------------------------------------
# Detect OS & package manager
# ---------------------------------------------------------------------------
header "Detecting OS"

OS="unknown"
PKG_MGR="unknown"
ARCH="$(uname -m)"

case "$(uname -s)" in
    Darwin)
        OS="macos"
        PKG_MGR="brew"
        info "Detected macOS ($(sw_vers -productVersion)) on ${ARCH}"
        ;;
    Linux)
        if [ -f /etc/os-release ]; then
            . /etc/os-release
            case "${ID:-}" in
                ubuntu|debian|pop|linuxmint|elementary)
                    OS="debian"
                    PKG_MGR="apt"
                    ;;
                fedora|rhel|centos|rocky|alma)
                    OS="fedora"
                    PKG_MGR="dnf"
                    ;;
                arch|manjaro|endeavouros)
                    OS="arch"
                    PKG_MGR="pacman"
                    ;;
                *)
                    # Fallback: try to detect the package manager
                    if has_cmd apt-get; then
                        OS="debian"; PKG_MGR="apt"
                    elif has_cmd dnf; then
                        OS="fedora"; PKG_MGR="dnf"
                    elif has_cmd pacman; then
                        OS="arch"; PKG_MGR="pacman"
                    fi
                    ;;
            esac
            info "Detected ${PRETTY_NAME:-Linux} on ${ARCH}"
        fi
        ;;
esac

if [ "$OS" = "unknown" ]; then
    fail "Unsupported operating system: $(uname -s)"
    echo "This script supports macOS, Debian/Ubuntu, Fedora/RHEL, and Arch Linux."
    exit 1
fi

# ---------------------------------------------------------------------------
# OS-specific install helpers
# ---------------------------------------------------------------------------
pkg_install() {
    local pkg_name="$1"
    info "Installing ${pkg_name}..."
    case "$PKG_MGR" in
        brew)   brew install "$pkg_name" ;;
        apt)    sudo apt-get install -y "$pkg_name" ;;
        dnf)    sudo dnf install -y "$pkg_name" ;;
        pacman) sudo pacman -S --noconfirm "$pkg_name" ;;
    esac
}

# Map a generic dependency name to the OS-specific package name
pkg_name_for() {
    local dep="$1"
    case "$dep" in
        node)
            case "$PKG_MGR" in
                brew)   echo "node" ;;
                apt)    echo "nodejs" ;;
                dnf)    echo "nodejs" ;;
                pacman) echo "nodejs" ;;
            esac
            ;;
        git)    echo "git" ;;
        d2)
            # d2 is only in Homebrew natively; Linux uses the install script
            echo "d2"
            ;;
        pandoc) echo "pandoc" ;;
        *)      echo "$dep" ;;
    esac
}

# ---------------------------------------------------------------------------
# Prerequisite checks — Node.js, npm, git (hard requirements)
# ---------------------------------------------------------------------------
header "Core Prerequisites"

# -- Node.js --
if has_cmd node; then
    NODE_VER="$(node -v)"
    NODE_MAJOR="${NODE_VER#v}"
    NODE_MAJOR="${NODE_MAJOR%%.*}"
    if [ "$NODE_MAJOR" -ge 20 ]; then
        check_pass "Node.js ${NODE_VER}"
    else
        warn "Node.js ${NODE_VER} found — v20+ recommended"
        if confirm "Install latest Node.js via ${PKG_MGR}?"; then
            pkg_install "$(pkg_name_for node)"
        fi
    fi
else
    check_fail "Node.js not found"
    if confirm "Install Node.js via ${PKG_MGR}?"; then
        pkg_install "$(pkg_name_for node)"
    else
        fail "Node.js is required. Aborting."
        exit 1
    fi
fi

# -- npm / npx --
# On some Linux distros (e.g. apt install nodejs), npm is a separate package.
if has_cmd npm && has_cmd npx; then
    check_pass "npm $(npm --version) / npx available"
else
    check_fail "npm/npx not found (pi requires npx for extension loading)"
    if [ "$PKG_MGR" = "apt" ]; then
        if confirm "Install npm via apt?"; then
            sudo apt-get install -y npm
        fi
    else
        warn "npm should be bundled with Node.js — try reinstalling Node"
    fi
fi

# -- git --
if has_cmd git; then
    check_pass "git $(git --version | awk '{print $3}')"
else
    check_fail "git not found"
    if confirm "Install git via ${PKG_MGR}?"; then
        pkg_install git
    else
        fail "git is required. Aborting."
        exit 1
    fi
fi

# -- pi coding agent --
if has_cmd pi || npx --yes @mariozechner/pi-coding-agent --version &>/dev/null; then
    check_pass "pi coding agent available"
else
    check_warn "pi coding agent not found"
    info "Install with: npm install -g @mariozechner/pi-coding-agent"
    if confirm "Install pi coding agent globally via npm?"; then
        npm install -g @mariozechner/pi-coding-agent
        if has_cmd pi; then
            check_pass "pi coding agent installed"
        fi
    fi
fi

# ---------------------------------------------------------------------------
# Ollama — Local Inference, Offline Mode, Semantic Memory Search
# ---------------------------------------------------------------------------
header "Ollama (Local Inference / Offline Mode / Semantic Memory)"

if has_cmd ollama; then
    check_pass "Ollama installed ($(ollama --version 2>/dev/null || echo 'version unknown'))"
else
    check_fail "Ollama not found"
    if confirm "Install Ollama?"; then
        if [ "$OS" = "macos" ]; then
            brew install --cask ollama
        else
            info "Installing Ollama via official install script..."
            curl -fsSL https://ollama.com/install.sh | sh
        fi
    fi
fi

# -- Ensure Ollama is running --
if has_cmd ollama; then
    if ollama list &>/dev/null; then
        check_pass "Ollama server is running"
    else
        warn "Ollama is installed but the server isn't running"
        if confirm "Start Ollama now?"; then
            if [ "$OS" = "macos" ]; then
                open -a Ollama
                info "Waiting for Ollama to start..."
                sleep 3
            else
                nohup ollama serve &>/dev/null &
                info "Waiting for Ollama to start..."
                sleep 3
            fi
            if ollama list &>/dev/null; then
                check_pass "Ollama server started"
            else
                check_warn "Ollama server may still be starting — try again in a moment"
            fi
        fi
    fi

    # -- Recommended models --
    if ollama list &>/dev/null; then
        MODELS="$(ollama list 2>/dev/null || true)"

        # Chat model (for local inference / offline driver)
        # Exclude embedding models — they can't be used for chat
        CHAT_MODELS="$(echo "$MODELS" | grep -ivE 'embed' || true)"
        if echo "$CHAT_MODELS" | grep -qiE "nemotron-3-nano|devstral-small|qwen3"; then
            CHAT_MODEL="$(echo "$CHAT_MODELS" | grep -oiE '(nemotron-3-nano|devstral-small-2|qwen3)[^ ]*' | head -1)"
            check_pass "Chat model available: ${CHAT_MODEL}"
        else
            check_warn "No recommended chat model found"
            info "Recommended (pick one based on your RAM):"
            info "  • ollama pull qwen3:8b          (8GB RAM)"
            info "  • ollama pull qwen3:30b          (32GB RAM)"
            info "  • ollama pull nemotron-3-nano:30b (32GB RAM)"
            if confirm "Pull qwen3:8b now (smallest)?"; then
                ollama pull qwen3:8b
            fi
        fi

        # Embedding model (for semantic memory search)
        if echo "$MODELS" | grep -qi "qwen3-embedding"; then
            EMBED_MODEL="$(echo "$MODELS" | grep -oiE 'qwen3-embedding[^ ]*' | head -1)"
            check_pass "Embedding model available: ${EMBED_MODEL}"
        else
            check_warn "No embedding model found (semantic memory search will use keyword fallback)"
            info "Recommended: ollama pull qwen3-embedding:0.6b  (~500MB)"
            if confirm "Pull qwen3-embedding:0.6b now?"; then
                ollama pull qwen3-embedding:0.6b
            fi
        fi
    fi
fi

# ---------------------------------------------------------------------------
# uv — Python project manager (needed for mflux & Excalidraw renderer)
# ---------------------------------------------------------------------------
header "uv (Python Project Manager)"

# uv installs to ~/.local/bin which may not be on PATH yet
if [ -d "$HOME/.local/bin" ]; then
    export PATH="$HOME/.local/bin:$PATH"
fi

if has_cmd uv; then
    check_pass "uv installed ($(uv --version 2>/dev/null))"
    ensure_local_bin_on_path
else
    check_fail "uv not found (needed for image generation & Excalidraw rendering)"
    if confirm "Install uv?"; then
        curl -LsSf https://astral.sh/uv/install.sh | sh
        # Make uv available for the rest of this script
        export PATH="$HOME/.local/bin:$PATH"
        if has_cmd uv; then
            check_pass "uv installed successfully"
            ensure_local_bin_on_path
        else
            check_warn "uv installed but not on PATH — restart your shell after this script"
        fi
    fi
fi

# ---------------------------------------------------------------------------
# D2 — Diagram rendering
# ---------------------------------------------------------------------------
header "D2 (Diagram Rendering)"

if has_cmd d2; then
    check_pass "d2 installed ($(d2 --version 2>/dev/null || echo 'version unknown'))"
else
    check_fail "d2 not found"
    if confirm "Install d2?"; then
        if [ "$OS" = "macos" ]; then
            brew install d2
        else
            info "Installing d2 via official install script..."
            curl -fsSL https://d2lang.com/install.sh | sh -s --
        fi
        if has_cmd d2; then
            check_pass "d2 installed successfully"
        fi
    fi
fi

# ---------------------------------------------------------------------------
# mflux — FLUX.1 image generation (Apple Silicon only)
# ---------------------------------------------------------------------------
header "mflux (FLUX.1 Image Generation)"

DIFFUSION_CLI_DIR="${DIFFUSION_CLI_DIR:-$HOME/diffusion-cli}"

if [ "$ARCH" = "arm64" ] && [ "$OS" = "macos" ]; then
    if [ -f "${DIFFUSION_CLI_DIR}/.venv/bin/mflux-generate" ]; then
        check_pass "mflux installed at ${DIFFUSION_CLI_DIR}"
    else
        check_fail "mflux not found at ${DIFFUSION_CLI_DIR}"
        if has_cmd uv; then
            if confirm "Set up mflux in ${DIFFUSION_CLI_DIR}?"; then
                info "Creating uv project and installing mflux..."
                if [ ! -d "$DIFFUSION_CLI_DIR" ]; then
                    uv init "$DIFFUSION_CLI_DIR"
                fi
                ( cd "$DIFFUSION_CLI_DIR" && uv add mflux )
                if [ -f "${DIFFUSION_CLI_DIR}/.venv/bin/mflux-generate" ]; then
                    check_pass "mflux installed successfully"
                else
                    check_warn "mflux installation may have failed — check ${DIFFUSION_CLI_DIR}"
                fi
            fi
        else
            check_warn "Install uv first (above), then re-run to set up mflux"
        fi
    fi
else
    check_warn "mflux requires Apple Silicon (arm64 macOS) — skipping on ${OS}/${ARCH}"
fi

# ---------------------------------------------------------------------------
# Excalidraw renderer — Playwright + Chromium
# ---------------------------------------------------------------------------
header "Excalidraw Renderer (Playwright + Chromium)"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
EXCALIDRAW_RENDER_DIR="${EXCALIDRAW_RENDER_DIR:-${SCRIPT_DIR}/extensions/render/excalidraw-renderer}"

if [ -d "$EXCALIDRAW_RENDER_DIR" ]; then
    if [ -d "${EXCALIDRAW_RENDER_DIR}/.venv" ]; then
        check_pass "Excalidraw renderer venv exists"
        # Check if Chromium is installed for Playwright
        if "${EXCALIDRAW_RENDER_DIR}/.venv/bin/python" -c "from playwright.sync_api import sync_playwright" 2>/dev/null; then
            check_pass "Playwright available in renderer venv"
        else
            check_warn "Playwright not fully set up in renderer venv"
        fi
    else
        check_fail "Excalidraw renderer not bootstrapped"
        if has_cmd uv; then
            if confirm "Set up Excalidraw renderer (uv sync + Playwright Chromium)?"; then
                ( cd "$EXCALIDRAW_RENDER_DIR" && uv sync && uv run playwright install chromium )
                check_pass "Excalidraw renderer bootstrapped"
            fi
        else
            check_warn "Install uv first (above), then re-run to set up Excalidraw renderer"
        fi
    fi
else
    check_warn "Excalidraw renderer directory not found at ${EXCALIDRAW_RENDER_DIR}"
fi

# ---------------------------------------------------------------------------
# pandoc — Document conversion for the view extension
# ---------------------------------------------------------------------------
header "pandoc (Document Conversion)"

if has_cmd pandoc; then
    check_pass "pandoc installed ($(pandoc --version | head -1))"
else
    check_warn "pandoc not found (view extension will skip rich doc rendering)"
    if confirm "Install pandoc?"; then
        pkg_install pandoc
        if has_cmd pandoc; then
            check_pass "pandoc installed successfully"
        fi
    fi
fi

# ---------------------------------------------------------------------------
# Web Search — API key checks
#
# pi stores secrets as resolution recipes in ~/.pi/agent/secrets.json.
# A recipe is a shell command (prefixed with !) that retrieves the value
# at runtime (e.g. from macOS Keychain). We check both env vars AND the
# pi secrets file to detect configured keys.
# ---------------------------------------------------------------------------
header "Web Search (API Keys)"

WEB_SEARCH_PROVIDERS=0
PI_SECRETS_FILE="$HOME/.pi/agent/secrets.json"

# Checks env var first, then pi secrets recipe file (exact key match)
has_secret() {
    local name="$1"
    # 1) Environment variable
    if [ -n "${!name:-}" ]; then
        return 0
    fi
    # 2) pi secrets.json recipe — match "KEY_NAME"\s*: to avoid substring false positives
    if [ -f "$PI_SECRETS_FILE" ] && grep -qE "\"${name}\"\\s*:" "$PI_SECRETS_FILE"; then
        return 0
    fi
    return 1
}

if has_secret "BRAVE_API_KEY"; then
    check_pass "BRAVE_API_KEY is configured"
    (( WEB_SEARCH_PROVIDERS++ )) || true
else
    check_fail "BRAVE_API_KEY not configured"
fi

if has_secret "TAVILY_API_KEY"; then
    check_pass "TAVILY_API_KEY is configured"
    (( WEB_SEARCH_PROVIDERS++ )) || true
fi

if has_secret "SERPER_API_KEY"; then
    check_pass "SERPER_API_KEY is configured"
    (( WEB_SEARCH_PROVIDERS++ )) || true
fi

if [ "$WEB_SEARCH_PROVIDERS" -eq 0 ]; then
    echo ""
    printf "  ${BLUE}ℹ${RESET}  ${BOLD}Brave Search is the default web search provider for this repo.${RESET}\n"
    printf "  ${BLUE}ℹ${RESET}  Tavily (https://tavily.com/) and Serper (https://serper.dev/) are also\n"
    printf "  ${BLUE}ℹ${RESET}  supported, but you will need to configure those on your own.\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  ${BOLD}To set up Brave Search:${RESET}\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  1. Go to ${CYAN}https://brave.com/search/api/${RESET} and create a Brave account\n"
    printf "  ${BLUE}ℹ${RESET}     (or sign in if you already have one).\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  2. Subscribe to the ${BOLD}Free${RESET} plan.\n"
    printf "  ${BLUE}ℹ${RESET}     Brave gives you ${GREEN}\$5/mo in free credits${RESET}. You can set a spending\n"
    printf "  ${BLUE}ℹ${RESET}     limit of \$0 in your account settings to ensure you are never charged.\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  3. Generate an API key from your Brave Search API dashboard.\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  4. In your next pi session, run:\n"
    echo ""
    printf "        ${CYAN}/secrets configure BRAVE_API_KEY${RESET}\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}     Paste your API key when prompted. pi will store it securely\n"
    printf "  ${BLUE}ℹ${RESET}     and make it available to the web search extension.\n"
    echo ""
    printf "  ${BLUE}ℹ${RESET}  5. Re-run this bootstrap script to verify:\n"
    echo ""
    printf "        ${CYAN}./bootstrap.sh${RESET}\n"
    echo ""
else
    info "${WEB_SEARCH_PROVIDERS}/3 web search provider(s) configured"
fi

# ---------------------------------------------------------------------------
# Validation Summary
# ---------------------------------------------------------------------------
header "Validation Summary"

printf "${GREEN}  Passed:   %d${RESET}\n" "$PASS"
printf "${YELLOW}  Warnings: %d${RESET}\n" "$WARN"
printf "${RED}  Failed:   %d${RESET}\n" "$FAIL"
echo ""

# -- Capability matrix --
printf "${BOLD}  Capability Status:${RESET}\n\n"

cap_status() {
    local name="$1" status="$2"
    case "$status" in
        ready)   printf "    ${GREEN}●${RESET} %-30s ${GREEN}Ready${RESET}\n" "$name" ;;
        partial) printf "    ${YELLOW}◐${RESET} %-30s ${YELLOW}Partial${RESET}\n" "$name" ;;
        missing) printf "    ${RED}○${RESET} %-30s ${RED}Not available${RESET}\n" "$name" ;;
    esac
}

# Cache ollama model list once for all capability checks
OLLAMA_MODELS=""
if has_cmd ollama; then
    OLLAMA_MODELS="$(ollama list 2>/dev/null || true)"
fi
OLLAMA_CHAT_MODELS="$(echo "$OLLAMA_MODELS" | grep -ivE 'embed' || true)"

# Project Memory
if [ -n "$OLLAMA_MODELS" ] && echo "$OLLAMA_MODELS" | grep -qi "qwen3-embedding"; then
    cap_status "Project Memory" "ready"
elif has_cmd ollama; then
    cap_status "Project Memory (keyword fallback)" "partial"
else
    cap_status "Project Memory (keyword fallback)" "partial"
fi

# Local Inference
if [ -n "$OLLAMA_CHAT_MODELS" ] && echo "$OLLAMA_CHAT_MODELS" | grep -qiE "nemotron|devstral|qwen3"; then
    cap_status "Local Inference" "ready"
elif has_cmd ollama; then
    cap_status "Local Inference (no models)" "partial"
else
    cap_status "Local Inference" "missing"
fi

# Offline Mode
if [ -n "$OLLAMA_CHAT_MODELS" ] && echo "$OLLAMA_CHAT_MODELS" | grep -qiE "nemotron|devstral|qwen3"; then
    cap_status "Offline Mode" "ready"
elif has_cmd ollama; then
    cap_status "Offline Mode (no models)" "partial"
else
    cap_status "Offline Mode" "missing"
fi

# Cleave
if has_cmd git && has_cmd node; then
    cap_status "Cleave (Task Decomposition)" "ready"
else
    cap_status "Cleave (Task Decomposition)" "missing"
fi

# Image Generation
if [ -f "${DIFFUSION_CLI_DIR}/.venv/bin/mflux-generate" ]; then
    cap_status "Image Generation (FLUX.1)" "ready"
elif [ "$ARCH" = "arm64" ] && [ "$OS" = "macos" ]; then
    cap_status "Image Generation (FLUX.1)" "missing"
else
    cap_status "Image Generation (Apple Silicon only)" "missing"
fi

# D2 Diagrams
if has_cmd d2; then
    cap_status "D2 Diagrams" "ready"
else
    cap_status "D2 Diagrams" "missing"
fi

# Excalidraw
if [ -d "${EXCALIDRAW_RENDER_DIR}/.venv" ]; then
    cap_status "Excalidraw Rendering" "ready"
else
    cap_status "Excalidraw Rendering" "missing"
fi

# Web Search
if [ "$WEB_SEARCH_PROVIDERS" -ge 1 ]; then
    cap_status "Web Search (${WEB_SEARCH_PROVIDERS}/3 providers)" "ready"
else
    cap_status "Web Search" "missing"
fi

# Model Budget
cap_status "Model Budget" "ready"

# View
if has_cmd pandoc; then
    cap_status "View (inline file viewer)" "ready"
else
    cap_status "View (no pandoc for docs)" "partial"
fi

# Utilities
cap_status "Utilities (chronos, whoami, etc.)" "ready"

echo ""

if [ "$FAIL" -eq 0 ]; then
    printf "${GREEN}${BOLD}  🎉 Bootstrap complete! All critical dependencies satisfied.${RESET}\n"
else
    printf "${YELLOW}${BOLD}  ⚡ Bootstrap complete with %d issue(s). Re-run to retry.${RESET}\n" "$FAIL"
fi

echo ""
