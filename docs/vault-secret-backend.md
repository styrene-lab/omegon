---
id: vault-secret-backend
title: Vault as first-class secret backend — operator-controlled secret storage with unseal lifecycle
status: implemented
parent: rust-agent-loop
tags: [vault, secrets, infrastructure, operator, ux]
open_questions: []
branches: ["feature/vault-secret-backend"]
openspec_change: vault-secret-backend
issue_type: feature
priority: 2
---

# Vault as first-class secret backend — operator-controlled secret storage with unseal lifecycle

## Overview

Elevate HashiCorp Vault from an incidental external tool to a first-class secret backend in the omegon-secrets crate. The operator should be able to choose to store a secret in Vault (not just env/keyring/file). The harness should be able to prompt the operator for unseal keys when Vault is sealed, manage the Vault lifecycle from the TUI, and resolve secrets from Vault paths as a recipe kind.

## Research

### Current secrets architecture

**omegon-secrets crate** has four layers:

1. **Resolution** (`resolve.rs`): priority chain env → recipe. Recipe kinds: `env:`, `cmd:`, `keyring:`/`keychain:`, `file:`. No Vault kind exists.
2. **Recipes** (`recipes.rs`): persisted in `~/.omegon/secrets.json` as name→recipe-string map. Stores *how* to resolve, never values. Already extensible — adding `vault:path/to/secret#key` is a natural addition.
3. **Redaction** (`redact.rs`): Aho-Corasick automaton scrubs resolved secret values from tool output. Backend-agnostic — works on resolved values regardless of source.
4. **Guards** (`guards.rs`): blocks/warns on tool calls touching sensitive paths. Already has a pattern for `vault read` commands (warns).

**Key insight**: the recipe system is the right integration point. A `vault:secret/data/bootstrap/ghcr/styrene-lab#password` recipe would resolve by calling the Vault HTTP API. The rest of the pipeline (redaction, guards, audit) works unchanged.

**Dependencies**: reqwest (already in the workspace for providers), serde_json (already). No new heavy deps needed — Vault's HTTP API is simple REST/JSON.

**What doesn't exist**:
- No Vault client in the crate
- No Vault address/token configuration
- No unseal workflow in the TUI
- No operator-facing secret storage choice UI
- No Vault health check in whoami or startup

### Integration surface — what needs to change

**Layer 1: Vault client** (`omegon-secrets/src/vault.rs`)
- HTTP client for Vault KV v2 API: read, write, list, health, seal-status
- Auth: token-based initially (root token or renewable service token)
- Address resolution: `VAULT_ADDR` env → recipe → `~/.omegon/vault.json` config
- Token resolution: `VAULT_TOKEN` env → `~/.vault-token` file → keyring → prompt
- Connection pooling via reqwest::Client (shared with providers)

**Layer 2: Recipe kind** (`resolve.rs`)
- New recipe kind: `vault:path#key` — resolves a single key from a KV v2 secret
- Example: `vault:secret/data/bootstrap/ghcr/styrene-lab#password`
- Falls back gracefully when Vault is unreachable (log warning, return None)

**Layer 3: Secret storage choice** (`recipes.rs` + TUI)
- When the operator stores a new secret (e.g., API key during login), offer backend choice: keyring (default), vault, env
- Store the recipe pointing to the chosen backend
- For Vault: agent writes the value to Vault KV, stores `vault:path#key` as the recipe

**Layer 4: Unseal lifecycle** (TUI feature)
- On startup or when Vault is detected as sealed: prompt operator for unseal keys
- Show seal status in dashboard (footer card or system notification)
- `/vault` slash command: status, unseal, seal, login
- Unseal keys entered via TUI input (masked, not echoed)
- Support partial unseal progress (3-of-5 threshold display)

**Layer 5: Health integration**
- `whoami` tool: include Vault status (unsealed/sealed/unreachable, address, auth method)
- Startup: check Vault health, warn if sealed, offer unseal
- Dashboard: Vault connection status badge

### Security boundaries and agent trust

**Critical constraint**: the agent must NOT have unseal keys or root tokens in its context. These are operator-only credentials.

**Trust model**:
- The **operator** holds unseal keys and can unseal Vault via the TUI prompt. The prompt is handled by the TUI layer, not the agent loop — the agent never sees the keys.
- The **agent** can *read* secrets from Vault via recipes (resolve a value for use in tool calls, e.g., API keys). It cannot *write* unseal keys, create root tokens, or modify Vault policies.
- The agent *can* write secrets to Vault if the operator has authorized it (e.g., storing an API key during setup). The guard system controls what paths the agent can access.
- The **redaction layer** scrubs any resolved Vault secret from tool output, same as keyring secrets.

**What the agent sees**: recipe strings like `vault:secret/data/myapp#api_key`. Never the resolved value in the system prompt — only in the resolved `SecretString` that passes through tool execution.

**What the operator controls**: Vault address, authentication, unseal, and the decision of which backend to use for each secret.

**Guard integration**: existing `vault read` bash command pattern already warns. Add a guard for the `vault:` recipe paths — warn if agent tries to read/edit `~/.omegon/vault.json` or secrets.json directly.

### Vault API surface needed

Vault's HTTP API is straightforward REST/JSON. The subset we need:

**Health & status** (unauthenticated):
- `GET /v1/sys/health` — returns sealed/standby/active status
- `GET /v1/sys/seal-status` — detailed seal state (threshold, progress, shares)

**Unseal** (unauthenticated):
- `PUT /v1/sys/unseal` — provide one unseal key, returns progress

**KV v2 read** (authenticated):
- `GET /v1/{mount}/data/{path}` — read secret data (version, metadata)
- `GET /v1/{mount}/metadata/{path}` — read metadata only

**KV v2 write** (authenticated):
- `POST /v1/{mount}/data/{path}` — create/update secret

**KV v2 list** (authenticated):
- `LIST /v1/{mount}/metadata/{path}` — list keys at path

**Auth** (for token renewal):
- `GET /v1/auth/token/lookup-self` — check current token status
- `POST /v1/auth/token/renew-self` — renew if renewable

Total: ~8 endpoints, all simple JSON request/response. No streaming, no SSE. A thin HTTP wrapper — not a full vault client crate.

**Crate choice**: implement directly with reqwest rather than pulling in a Vault client crate. The API surface is small, we already have reqwest, and it avoids a heavy dependency for 8 endpoints.

### Path restriction model — Vault policy as the agent permission boundary

**The insight**: Vault already has a fine-grained policy system. Instead of building a separate ACL layer in omegon, the operator mints a Vault token with a policy that restricts exactly which paths the agent can read/write. The omegon harness authenticates with that scoped token — not a root token — and Vault itself enforces the boundary.

**Three deployment modes, one mechanism**:

1. **In-cluster pod**: Kubernetes auth method. The pod's ServiceAccount maps to a Vault role with a policy. The agent never sees a token — Vault issues one via SA JWT exchange. Paths are locked by the role's policy. This is what VSO does today, but the agent would use the same auth path to read secrets directly instead of relying on pre-synced K8s Secrets.

2. **Operator laptop**: The operator runs `/vault login` or provides a token. The operator can either:
   - Use a scoped token directly (`vault token create -policy=omegon-agent -ttl=8h`)
   - Use AppRole auth — omegon stores a role_id + secret_id, exchanges them for a scoped token at startup
   - Use OIDC auth — browser-based flow, returns a token scoped to the operator's Vault identity/policies

3. **Headless long-running machine**: AppRole is the natural fit. The operator provisions `role_id` (stable, stored in config) + `secret_id` (rotatable, stored in keyring or injected via env). Omegon exchanges them on startup, gets a renewable token, auto-renews. If the token expires and can't renew, it falls back to prompting for re-auth or degrading gracefully.

**What the policy looks like** (operator-authored, stored in Vault):

```hcl
# omegon-agent policy — scoped to specific secret paths
path "secret/data/omegon/*" {
  capabilities = ["read", "create", "update"]
}
path "secret/metadata/omegon/*" {
  capabilities = ["read", "list"]
}
# Read-only access to shared infrastructure secrets
path "secret/data/bootstrap/ghcr/*" {
  capabilities = ["read"]
}
# Deny everything else implicitly (Vault default-deny)
```

**Runtime restriction via the harness**: Even with a valid Vault token, omegon can add a client-side allowlist in `~/.omegon/vault.json`:

```json
{
  "addr": "https://vault.vanderlyn.house",
  "auth": "approle",
  "allowed_paths": ["secret/data/omegon/*", "secret/data/bootstrap/ghcr/*"],
  "denied_paths": ["secret/data/bootstrap/cloudflare/*"]
}
```

This is defense-in-depth: Vault enforces server-side via policy, omegon enforces client-side via allowlist. The agent can't trick the harness into reading a path that's not in the allowlist, and even if it did, Vault would reject it if the token's policy doesn't cover it.

**For multi-agent orchestration**: each cleave child gets its own scoped token (or inherits the parent's). The parent can mint child tokens with narrower policies via `vault token create -policy=omegon-child-{task} -ttl=30m -use-limit=100`. When the child finishes, the token expires. The parent's policy needs `create` on `auth/token/create` with `allowed_policies` constraining what it can delegate.

**What omegon needs to implement**:
- Auth method negotiation: token → AppRole → K8s SA → OIDC (fallback chain)
- Client-side path allowlist/denylist in vault.json
- Token lifecycle: acquire → cache → renew → re-acquire on expiry
- Child token minting for cleave children (parent creates scoped child tokens)
- Policy template generation: `/vault init-policy` outputs a starter HCL policy the operator can customize and apply to Vault

## Decisions

### Decision: Vault policy is the primary agent permission boundary — dual enforcement with client-side allowlist

**Status:** decided
**Rationale:** Vault's policy system already does fine-grained path ACLs with default-deny. Building a parallel ACL in omegon would be redundant and diverge from how every other Vault consumer works. The operator authors a Vault policy (omegon can generate a starter template), attaches it to the agent's auth method, and Vault enforces server-side. Omegon adds a client-side allowlist in vault.json as defense-in-depth — the agent can't reach paths not in the allowlist even if the token technically permits them. Two layers: Vault says no → blocked. Omegon says no → blocked. Both must say yes.

### Decision: Support token, AppRole, and Kubernetes SA auth — fallback chain per deployment mode

**Status:** decided
**Rationale:** Three deployment modes need three auth methods. In-cluster: K8s SA auth (zero-config, pod identity). Laptop: token or OIDC (operator-interactive). Headless: AppRole (role_id in config + secret_id in keyring, auto-renewable). The client negotiates: if K8s SA JWT exists → K8s auth. Else if AppRole config exists → AppRole. Else if VAULT_TOKEN or ~/.vault-token → token. Else prompt operator. All methods produce a scoped Vault token that the recipe resolver uses.

### Decision: Cleave children receive scoped child tokens minted by the parent

**Status:** decided
**Rationale:** When a cleave parent spawns children, each child gets a Vault token with narrower policy, shorter TTL, and optional use-limit. The parent's policy must include auth/token/create with allowed_policies. When the child finishes or times out, the token expires. This gives each parallel task the minimum secret access it needs and provides natural cleanup — no orphaned credentials from crashed children.

### Decision: Vault client lives in omegon-secrets, unseal UI stays in the binary crate

**Status:** decided
**Rationale:** The HTTP client, auth negotiation, token lifecycle, and path allowlist are secret-resolution concerns — they belong in omegon-secrets alongside keyring and recipes. The TUI unseal prompt and /vault command are UI concerns that live in the binary crate and call into omegon-secrets methods. No new crate needed — the 8-endpoint Vault API doesn't justify the workspace overhead. The boundary is: omegon-secrets exports VaultClient with unseal(), status(), read(), write(). The TUI calls these and handles masked input.

### Decision: Auto-detect Vault with explicit override — VAULT_ADDR → vault.json → skip

**Status:** decided
**Rationale:** Check VAULT_ADDR env first (standard Vault convention, works everywhere). Then check ~/.omegon/vault.json for persisted config. Don't probe localhost:8200 blindly — false positives on machines running Vault for other purposes. If neither is set, Vault features are disabled silently. The operator enables Vault explicitly via /vault configure or by setting VAULT_ADDR.

### Decision: Non-blocking degradation when sealed — notify and offer /vault unseal, don't gate startup

**Status:** decided
**Rationale:** Omegon must be usable without Vault. If Vault is configured but sealed: show a system notification ("Vault is sealed — secrets from Vault unavailable. Use /vault unseal"), degrade vault: recipes to None (logged), and continue. The operator can unseal at any time during the session. Blocking startup would make Vault a mandatory dependency which it isn't — env vars and keyring still work.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `crates/omegon-secrets/src/vault.rs` (new) — Vault HTTP client — health, seal-status, unseal, KV v2 read/write/list, token lookup/renew
- `crates/omegon-secrets/src/resolve.rs` (modified) — Add vault: recipe kind — resolves vault:path#key via Vault client
- `crates/omegon-secrets/src/lib.rs` (modified) — Wire VaultClient into SecretsManager, add vault config, expose unseal/status methods
- `crates/omegon-secrets/Cargo.toml` (modified) — Add reqwest + tokio dependency for async Vault HTTP calls
- `crates/omegon/src/tui/mod.rs` (modified) — Add /vault slash command (status, unseal, seal, login), masked unseal key input
- `crates/omegon/src/tools/whoami.rs` (modified) — Include Vault status in whoami output
- `crates/omegon/src/features/lifecycle.rs` (modified) — Vault health check in startup context injection
- `crates/omegon-secrets/src/guards.rs` (modified) — Add guard patterns for vault config files
- `crates/omegon-secrets/src/vault.rs` (new) — Vault HTTP client: health, seal-status, unseal, KV v2 CRUD, token lifecycle, auth method negotiation (token/AppRole/K8s SA), path allowlist enforcement, child token minting for cleave
- `crates/omegon/src/tui/mod.rs` (modified) — /vault command: configure, status, unseal (masked multi-key input with progress), login, init-policy. Operator-only — agent loop never invokes these.

### Constraints

- Agent must NEVER see unseal keys or root tokens in its context — these are operator-only credentials handled by the TUI layer
- Vault client must degrade gracefully — unreachable Vault returns None from recipe resolution, does not crash
- Unseal key input must be masked in the TUI — not echoed, not logged, not stored in session history
- Resolved Vault secrets must flow through the existing redaction pipeline — no special cases
- Vault configuration (address, token) stored in ~/.omegon/vault.json — never in the recipe file or session state
- Vault policy is the primary enforcement — omegon MUST NOT bypass or weaken server-side Vault ACLs
- Client-side allowlist in vault.json is defense-in-depth — agent paths checked against allowlist before any Vault API call
- Cleave child tokens must have narrower policy, shorter TTL, and optional use-limit compared to parent token
- /vault init-policy must generate a starter HCL policy file the operator can review and apply — omegon never writes Vault policies directly
