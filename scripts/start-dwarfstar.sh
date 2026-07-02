#!/usr/bin/env bash
set -euo pipefail

# Happy-path launcher for a local DwarfStar/ds4 OpenAI-compatible server.
# After this is running, Omegon can consume it with: om -m deepseek-local

ROOT="${DS4_ROOT:-$HOME/models/ds4}"
HOST="${DS4_HOST:-127.0.0.1}"
PORT="${DS4_PORT:-8000}"
CTX="${DS4_CTX:-32768}"
TOKENS="${DS4_TOKENS:-1024}"
KV_DIR="${DS4_KV_DIR:-/tmp/ds4-kv}"
KV_MB="${DS4_KV_MB:-8192}"
MODEL="${DS4_MODEL:-$ROOT/ds4flash.gguf}"
MIN_RAM_GB="${OMEGON_DWARFSTAR_MIN_RAM_GB:-96}"

fail() {
  echo "start-dwarfstar: $*" >&2
  exit 1
}

if command -v sysctl >/dev/null 2>&1; then
  mem_bytes="$(sysctl -n hw.memsize 2>/dev/null || true)"
else
  mem_bytes=""
fi

if [ -z "$mem_bytes" ] && [ -r /proc/meminfo ]; then
  mem_kb="$(awk '/^MemTotal:/ {print $2}' /proc/meminfo)"
  mem_bytes="$((mem_kb * 1024))"
fi

if [ -z "$mem_bytes" ]; then
  fail "could not detect host RAM; set OMEGON_DWARFSTAR_MIN_RAM_GB=0 to bypass the guard explicitly"
fi

mem_gb="$((mem_bytes / 1024 / 1024 / 1024))"
if [ "$mem_gb" -lt "$MIN_RAM_GB" ]; then
  fail "requires at least ${MIN_RAM_GB}GiB RAM for this experimental local provider; detected ${mem_gb}GiB"
fi

[ -d "$ROOT" ] || fail "ds4 root not found: $ROOT"
[ -x "$ROOT/ds4-server" ] || fail "ds4-server not executable: $ROOT/ds4-server"
[ -f "$MODEL" ] || fail "model file not found: $MODEL"

mkdir -p "$KV_DIR"

if curl -fsS "http://${HOST}:${PORT}/v1/models" >/dev/null 2>&1; then
  echo "DwarfStar already reachable at http://${HOST}:${PORT}/v1"
  echo "Use: om -m deepseek-local"
  exit 0
fi

cat >&2 <<EOF
Starting DwarfStar/ds4 server:
  root:   $ROOT
  model:  $MODEL
  url:    http://${HOST}:${PORT}/v1
  ctx:    $CTX
  tokens: $TOKENS
  kv:     $KV_DIR (${KV_MB}MiB)

Consume from Omegon with:
  om -m deepseek-local
EOF

cd "$ROOT"
exec "$ROOT/ds4-server" \
  --host "$HOST" \
  --port "$PORT" \
  --model "$MODEL" \
  --ctx "$CTX" \
  --tokens "$TOKENS" \
  --kv-disk-dir "$KV_DIR" \
  --kv-disk-space-mb "$KV_MB"
