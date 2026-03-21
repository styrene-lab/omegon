# Unified auth surface — single /login command, agent-callable, all backends — Design Spec (extracted)

> Auto-extracted from docs/unified-auth-surface.md at decide-time.

## Decisions

### /vault stays as separate power-user command — /auth covers login/status, /vault covers operations (decided)

/auth handles the common auth surface: status, login, logout, unlock across all backends. /vault stays for Vault-specific operations (unseal, configure, init-policy) that don't map to the generic auth model. /auth login vault triggers the same flow as /vault login — one calls the other internally.

### Vault as upstream secret backend for LLM API keys — vault:secret/data/omegon/anthropic recipe type (decided)

Power users who keep all secrets in Vault should be able to resolve LLM provider API keys from Vault paths instead of env vars or OAuth. The secret recipe system already supports shell commands — add a 'vault' recipe type that reads from Vault KV paths. When Vault is configured and unsealed, resolve_api_key checks the recipe store before falling back to env/OAuth. This means an operator with Vault can run Omegon without any local API keys or OAuth tokens — everything comes from Vault.

### Enable MCP Streamable HTTP + OAuth now — rmcp transport-streamable-http-client-reqwest + auth features (decided)

Remote MCP servers with OAuth are the standard pattern for hosted tool services (GitHub, Vercel, etc). OpenCode supports this natively. The rmcp crate already implements the transport and OAuth flow — we just need to enable the features and add a 'url' field to McpServerConfig. Deferring means operators can't use any remote MCP server, which is a growing gap as the MCP ecosystem shifts toward HTTP.

## Research Summary

### Current state — 6 auth mechanisms, 3 UX paths

**LLM Providers (auth.rs)**
- Anthropic OAuth: PKCE flow → `~/.pi/agent/auth.json["anthropic"]`
- OpenAI OAuth: PKCE flow → `~/.pi/agent/auth.json["openai"]`
- CLI only: `omegon login anthropic` / `omegon login openai`
- No TUI command, no agent tool
- Token refresh on expiry is automatic in `resolve_api_key_sync()`
- GitHub Copilot: managed by pi-ai internally, no Omegon-side auth

**Vault (omegon-secrets/vault.rs)**
- Token, AppRole, Kubernetes SA auth methods
- TUI only: `/vault login`, `/vau…

### Proposed unified surface

**Three entry points, one backend:**

### 1. CLI: `omegon auth <action> [provider]`

```
omegon auth status              # show all auth states
omegon auth login anthropic     # OAuth flow
omegon auth login openai        # OAuth flow
omegon auth login vault         # Vault token/AppRole auth
omegon auth unlock secrets      # unlock encrypted store
omegon auth logout anthropic    # revoke + remove token
```

Replaces: `omegon login <provider>` (backward compat alias kept).

### 2. TUI: `/auth [action] [provider]`

```
/auth                           # show auth status table
/auth login anthropic           # trigger OAuth (opens browser)
/auth login vault               # prompt for Vault token
/auth unlock                    # unlock secrets store (prompt for passphrase)
/auth logout openai             # revoke token
```

Replaces: `/vault login`, `/vault status` (vault becomes a sub-surface of auth).

### 3. Agent tool: `auth_status`

```json
{"action": "status"}            // returns all provider auth states
{"action": "check", "provider": "anthropic"}  // check specific provider
```

Read-only — the agent can check auth status but cannot trigger login flows (those require operator interaction). The agent CAN detect "auth expired" and suggest `/auth login <provider>` to the operator via BusRequest::Notify.

### Auth status table format

```
Provider      Status     Method    Expires
─────────────────────────────────────────────
Anthropic     ✓ active   OAuth     2h 15m
OpenAI        ✓ active   API key   never
Vault         ✓ unsealed Token     session
Secrets       🔒 locked  keyring   —
MCP:github    ✗ expired  OAuth     —3m ago
```

### HarnessStatus integration

The `providers` field in HarnessStatus (currently empty at startup) should be populated from this unified auth check. The bootstrap panel and footer already render it — they just need real data.

### Storage consolidation

All auth tokens stay where they are (auth.json, vault.json, secrets.db, env vars). The unified surface is a *read* layer that probes each backend and presents a coherent view. No storage migration needed.
