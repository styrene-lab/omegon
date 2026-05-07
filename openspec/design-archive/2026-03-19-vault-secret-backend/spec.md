+++
id = "7dfebf77-f07c-440b-b65a-44a9656b03b4"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vault as first-class secret backend — operator-controlled secret storage with unseal lifecycle — Design Spec (extracted)

> Auto-extracted from docs/vault-secret-backend.md at decide-time.

## Decisions

### Vault policy is the primary agent permission boundary — dual enforcement with client-side allowlist (decided)

Vault's policy system already does fine-grained path ACLs with default-deny. Building a parallel ACL in omegon would be redundant and diverge from how every other Vault consumer works. The operator authors a Vault policy (omegon can generate a starter template), attaches it to the agent's auth method, and Vault enforces server-side. Omegon adds a client-side allowlist in vault.json as defense-in-depth — the agent can't reach paths not in the allowlist even if the token technically permits them. Two layers: Vault says no → blocked. Omegon says no → blocked. Both must say yes.

### Support token, AppRole, and Kubernetes SA auth — fallback chain per deployment mode (decided)

Three deployment modes need three auth methods. In-cluster: K8s SA auth (zero-config, pod identity). Laptop: token or OIDC (operator-interactive). Headless: AppRole (role_id in config + secret_id in keyring, auto-renewable). The client negotiates: if K8s SA JWT exists → K8s auth. Else if AppRole config exists → AppRole. Else if VAULT_TOKEN or ~/.vault-token → token. Else prompt operator. All methods produce a scoped Vault token that the recipe resolver uses.

### Cleave children receive scoped child tokens minted by the parent (decided)

When a cleave parent spawns children, each child gets a Vault token with narrower policy, shorter TTL, and optional use-limit. The parent's policy must include auth/token/create with allowed_policies. When the child finishes or times out, the token expires. This gives each parallel task the minimum secret access it needs and provides natural cleanup — no orphaned credentials from crashed children.

### Vault client lives in omegon-secrets, unseal UI stays in the binary crate (decided)

The HTTP client, auth negotiation, token lifecycle, and path allowlist are secret-resolution concerns — they belong in omegon-secrets alongside keyring and recipes. The TUI unseal prompt and /vault command are UI concerns that live in the binary crate and call into omegon-secrets methods. No new crate needed — the 8-endpoint Vault API doesn't justify the workspace overhead. The boundary is: omegon-secrets exports VaultClient with unseal(), status(), read(), write(). The TUI calls these and handles masked input.

### Auto-detect Vault with explicit override — VAULT_ADDR → vault.json → skip (decided)

Check VAULT_ADDR env first (standard Vault convention, works everywhere). Then check ~/.omegon/vault.json for persisted config. Don't probe localhost:8200 blindly — false positives on machines running Vault for other purposes. If neither is set, Vault features are disabled silently. The operator enables Vault explicitly via /vault configure or by setting VAULT_ADDR.

### Non-blocking degradation when sealed — notify and offer /vault unseal, don't gate startup (decided)

Omegon must be usable without Vault. If Vault is configured but sealed: show a system notification ("Vault is sealed — secrets from Vault unavailable. Use /vault unseal"), degrade vault: recipes to None (logged), and continue. The operator can unseal at any time during the session. Blocking startup would make Vault a mandatory dependency which it isn't — env vars and keyring still work.

## Research Summary

### Current secrets architecture

**omegon-secrets crate** has four layers:

1. **Resolution** (`resolve.rs`): priority chain env → recipe. Recipe kinds: `env:`, `cmd:`, `keyring:`/`keychain:`, `file:`. No Vault kind exists.
2. **Recipes** (`recipes.rs`): persisted in `~/.omegon/secrets.json` as name→recipe-string map. Stores *how* to resolve, never values. Already extensible — adding `vault:path/to/secret#key` is a natural addition.
3. **Redaction** (`redact.rs`): Aho-Corasick automaton scrubs resolved secret values from tool o…

### Integration surface — what needs to change

**Layer 1: Vault client** (`omegon-secrets/src/vault.rs`)
- HTTP client for Vault KV v2 API: read, write, list, health, seal-status
- Auth: token-based initially (root token or renewable service token)
- Address resolution: `VAULT_ADDR` env → recipe → `~/.omegon/vault.json` config
- Token resolution: `VAULT_TOKEN` env → `~/.vault-token` file → keyring → prompt
- Connection pooling via reqwest::Client (shared with providers)

**Layer 2: Recipe kind** (`resolve.rs`)
- New recipe kind: `vault:path#…

### Security boundaries and agent trust

**Critical constraint**: the agent must NOT have unseal keys or root tokens in its context. These are operator-only credentials.

**Trust model**:
- The **operator** holds unseal keys and can unseal Vault via the TUI prompt. The prompt is handled by the TUI layer, not the agent loop — the agent never sees the keys.
- The **agent** can *read* secrets from Vault via recipes (resolve a value for use in tool calls, e.g., API keys). It cannot *write* unseal keys, create root tokens, or modify Vault p…

### Vault API surface needed

Vault's HTTP API is straightforward REST/JSON. The subset we need:

**Health & status** (unauthenticated):
- `GET /v1/sys/health` — returns sealed/standby/active status
- `GET /v1/sys/seal-status` — detailed seal state (threshold, progress, shares)

**Unseal** (unauthenticated):
- `PUT /v1/sys/unseal` — provide one unseal key, returns progress

**KV v2 read** (authenticated):
- `GET /v1/{mount}/data/{path}` — read secret data (version, metadata)
- `GET /v1/{mount}/metadata/{path}` — read metadat…

### Path restriction model — Vault policy as the agent permission boundary

**The insight**: Vault already has a fine-grained policy system. Instead of building a separate ACL layer in omegon, the operator mints a Vault token with a policy that restricts exactly which paths the agent can read/write. The omegon harness authenticates with that scoped token — not a root token — and Vault itself enforces the boundary.

**Three deployment modes, one mechanism**:

1. **In-cluster pod**: Kubernetes auth method. The pod's ServiceAccount maps to a Vault role with a policy. The a…
