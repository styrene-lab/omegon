# Chaos Proxy Evaluation Results — Pass 1

## Test Infrastructure

- **Chaos proxy**: `evalmonkey/scripts/chaos_proxy.py` — transparent HTTP proxy with provider-aware error injection
- **Error format**: Auto-detects Anthropic (x-api-key/anthropic-version) vs OpenAI (Bearer) from request headers
- **Adapter**: `evalmonkey/apps/framework_adapters/omegon_adapter.py` — routes omegon's API calls through proxy via `OMEGON_CHAOS_PROXY` env var
- **Auth**: Anthropic OAuth subscription token (Claude Code `~/.claude.json` adoption)
- **Omegon binary**: 0.18.0 release build (ed933162)

## Results: Anthropic Provider (Pass 1)

### Pre-forward profiles (block request before reaching API)

| Profile | Method | Score/Behavior | Retries | Correct |
|---|---|---|---|---|
| `rate_limit_429` | Direct | Clean "upstream exhausted" exit | 5x backoff (750→1500→3000ms) | **Yes** — retries transient, exits cleanly |
| `server_error_500` | Direct | Clean "upstream exhausted" exit | 5x backoff | **Yes** |
| `overloaded_529` | Direct | "provider overloaded", clean exit | 5x backoff | **Yes** — distinct classification from 5xx |
| `auth_revoke` (401) | Direct | Immediate failure, no retry | 0 | **Yes** — auth errors are not transient |
| `request_too_large_413` | Direct | Immediate failure, no retry | 0 | **Yes** — payload errors are not transient |
| `timeout_504` | Direct | Retries as transient "upstream 5xx" | 5x backoff | **Yes** |
| `intermittent_failure` | Full pipeline (evalmonkey) | **100/100** | Retries through 50%, succeeds | **Yes** — resilient |

### Post-forward profiles (mutate response after real API call)

| Profile | Method | Behavior | Correct |
|---|---|---|---|
| `empty_response` | Direct | Detects incomplete stream, retries 5x, "bridge dropped stream" | **Yes** |
| `corrupt_json` | Direct | Recovers partial content from corrupted response | **Yes** — resilient |
| `partial_stream_cut` | Direct | Detects truncation, retries 5x, "bridge dropped stream" | **Yes** |

### Timing profiles

| Profile | Method | Behavior | Correct |
|---|---|---|---|
| `latency_spike` | Direct | Waits patiently (5-30s delay) | **Yes** |
| `timeout_no_response` | Direct | Waits until subprocess timeout (300s) | **Yes** — but slow |

### Infrastructure profiles (not testable via proxy architecture)

| Profile | Why | Alternative |
|---|---|---|
| `hallucinated_tool` | Tests inter-tool validation; adapter wraps entire agent | Would need internal tool-result injection |
| `model_downgrade` | Tests quality detection; proxy can only truncate | Would need provider-level model swap |
| `memory_amnesia` | Tests session recovery; adapter runs fresh per request | Would need session-persistent testing |

## Key Findings

### No omegon bugs found
Every chaos profile produced correct behavior:
- Transient errors (429, 500, 504, 529) → retry with exponential backoff
- Permanent errors (401, 413) → immediate failure, no retry
- Incomplete/corrupt responses → detected and retried
- Intermittent failures → retried through to success (100/100 on eval)

### Error classification is correct
- `rate_limit_429` → classified as "rate-limited"
- `server_error_500` → classified as "upstream 5xx"
- `overloaded_529` → classified as "provider overloaded"
- `auth_revoke_401` → classified as fatal (no retry)
- `timeout_504` → classified as "upstream 5xx" (retryable)

### OAuth token handling
- OAuth tokens survive proxying (confirmed with passthrough)
- Tokens expire frequently (~2hr) and need refresh before each run
- The `anthropic-beta: oauth-2025-04-20` header is required for OAuth auth
- Rate limits on OAuth subscription are stricter than API keys

## Remaining Providers for Pass 1

### OpenAI (openai, openai-codex)
- Proxy target: `https://api.openai.com`
- Auth: Bearer token via `OPENAI_API_KEY`
- Error format: `{"error":{"message":"...","type":"...","code":"..."}}`
- Base URL env: `OPENAI_BASE_URL` (need to verify in providers.rs)

### OpenAI-Compatible (groq, xai, mistral, cerebras, perplexity)
- Each has its own base URL (via `compat_base_url()`)
- All use Bearer auth, OpenAI error format
- Base URL overridable via provider-specific env vars

### Ollama (local)
- No proxy needed — runs locally
- Error format: Ollama-specific
- Rate limits: none (local)

## Reproduction

```bash
# Start proxy
cd evalmonkey && python scripts/chaos_proxy.py --port 9999 --profile <profile>

# Start adapter with proxy routing
OMEGON_BIN=target/release/omegon \
OMEGON_CHAOS_PROXY=http://localhost:9999 \
OMEGON_MAX_RETRIES=5 \
python apps/framework_adapters/omegon_adapter.py --port 8321

# Run eval
ANTHROPIC_API_KEY=<token> EVAL_MODEL=anthropic/claude-haiku-4-5 \
evalmonkey run-benchmark --scenario gsm8k \
  --target-url http://localhost:8321/chat \
  --request-key message --response-path reply --limit 3
```
