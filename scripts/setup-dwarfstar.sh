#!/usr/bin/env bash
set -euo pipefail

# One-command happy-path setup for Omegon's experimental local DwarfStar/ds4
# provider. This intentionally does not vendor ds4 or model weights into Omegon;
# it clones/builds/downloads under DS4_ROOT and prints the single run command.

ROOT="${DS4_ROOT:-$HOME/models/ds4}"
REPO_URL="${DS4_REPO_URL:-https://github.com/antirez/ds4.git}"
REF="${DS4_REF:-main}"
QUANT="${DS4_QUANT:-q2-imatrix}"
MIN_RAM_GB="${OMEGON_DWARFSTAR_MIN_RAM_GB:-96}"
SKIP_DOWNLOAD="${DS4_SKIP_DOWNLOAD:-0}"
START_SCRIPT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/start-dwarfstar.sh"

fail() {
  echo "setup-dwarfstar: $*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"
}

ram_bytes() {
  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.memsize 2>/dev/null || true
    return
  fi
  if [ -r /proc/meminfo ]; then
    awk '/^MemTotal:/ {print $2 * 1024}' /proc/meminfo
  fi
}

mem_bytes="$(ram_bytes)"
if [ -z "$mem_bytes" ]; then
  fail "could not detect host RAM; set OMEGON_DWARFSTAR_MIN_RAM_GB=0 to bypass the guard explicitly"
fi
mem_gb="$((mem_bytes / 1024 / 1024 / 1024))"
if [ "$mem_gb" -lt "$MIN_RAM_GB" ]; then
  fail "requires at least ${MIN_RAM_GB}GiB RAM for this experimental local provider; detected ${mem_gb}GiB"
fi

need_cmd git
need_cmd make
need_cmd curl

mkdir -p "$(dirname "$ROOT")"

if [ ! -d "$ROOT/.git" ]; then
  git clone "$REPO_URL" "$ROOT"
fi

cd "$ROOT"
git fetch --tags origin
if [ "$REF" = "main" ]; then
  git checkout main
  git pull --ff-only origin main
else
  git checkout "$REF"
fi

make

if [ "$SKIP_DOWNLOAD" != "1" ]; then
  [ -x ./download_model.sh ] || fail "download_model.sh not found or not executable in $ROOT"
  ./download_model.sh "$QUANT"
else
  echo "Skipping model download because DS4_SKIP_DOWNLOAD=1"
fi

[ -x ./ds4-server ] || fail "build did not produce executable ds4-server"
[ -f ./ds4flash.gguf ] || fail "model symlink/file ds4flash.gguf was not found after setup"

cat <<EOF
DwarfStar setup complete.

Start server:
  DS4_CTX=100000 DS4_TOKENS=1024 "$START_SCRIPT"

Use from Omegon:
  om -m deepseek-local

Installed at:
  $ROOT
EOF
