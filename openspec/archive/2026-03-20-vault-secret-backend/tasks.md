# Vault as first-class secret backend — Tasks

## 1. Vault HTTP client + config (core/crates/omegon-secrets)

**Scope**: `src/vault.rs` (new), `Cargo.toml` (modified), `src/lib.rs` (modified)

- [x] 1.1 Add reqwest + tokio to omegon-secrets/Cargo.toml
- [x] 1.2 VaultConfig struct — addr, auth method, allowed_paths, denied_paths. Load from `~/.omegon/vault.json`, fallback to VAULT_ADDR env
- [x] 1.3 VaultClient::new(config) — reqwest client, token cache (Arc<RwLock<Option<String>>>)
- [x] 1.4 Health + seal status — `GET /v1/sys/health`, `GET /v1/sys/seal-status`. Returns typed structs.
- [x] 1.5 Unseal — `PUT /v1/sys/unseal` with one key, returns progress (sealed, threshold, progress, nonce)
- [x] 1.6 Auth negotiation — try in order: VAULT_TOKEN env → ~/.vault-token file → keyring("vault-token") → AppRole exchange → K8s SA JWT exchange. Result: cached token string.
- [x] 1.7 AppRole auth — `POST /v1/auth/approle/login` with role_id + secret_id (secret_id from keyring)
- [x] 1.8 K8s SA auth — `POST /v1/auth/kubernetes/login` with role + JWT from `/var/run/secrets/kubernetes.io/serviceaccount/token`
- [x] 1.9 Token lifecycle — `GET /v1/auth/token/lookup-self`, `POST /v1/auth/token/renew-self`. Background renew before TTL expiry.
- [x] 1.10 KV v2 read — `GET /v1/{mount}/data/{path}`, extract `.data.data` map. Check path against allowlist/denylist before call.
- [x] 1.11 KV v2 write — `POST /v1/{mount}/data/{path}` with `{"data": {...}}`. Check path against allowlist/denylist.
- [x] 1.12 KV v2 list — `LIST /v1/{mount}/metadata/{path}`, returns key names.
- [x] 1.13 Path allowlist/denylist enforcement — check requested path against vault.json `allowed_paths` (glob) and `denied_paths` (glob) before any API call. Deny if not matched.
- [x] 1.14 Child token minting — `POST /v1/auth/token/create` with policies, ttl, use_limit. For cleave child dispatch.
- [x] 1.15 Wire VaultClient into SecretsManager — optional field, initialized from VaultConfig if configured, None otherwise
- [x] 1.16 Tests: mock HTTP responses for health, read, write, auth negotiation, path allowlist enforcement

## 2. Vault recipe kind (core/crates/omegon-secrets/src/resolve.rs)

- [x] 2.1 Add `vault:` recipe kind — parse `vault:path#key` (e.g., `vault:secret/data/omegon/api-keys#anthropic`)
- [x] 2.2 Resolve by calling VaultClient.read(path), extract the named key from the data map
- [x] 2.3 Graceful fallback — if VaultClient is None or unreachable, log warning and return None
- [x] 2.4 Tests: vault recipe parsing, resolution with mock client, fallback on unreachable

## 3. Guard patterns (core/crates/omegon-secrets/src/guards.rs)

- [x] 3.1 Add Block pattern for `~/.omegon/vault.json` (Vault config — contains token/auth)
- [x] 3.2 Add Warn pattern for `.vault-token` (already exists — verify)
- [x] 3.3 Tests: guard blocks vault.json read, warns on vault-token

## 4. /vault TUI command (core/crates/omegon/src/tui/mod.rs)

- [x] 4.1 `/vault status` — call VaultClient.health(), display sealed/unsealed/unreachable, addr, auth method
- [x] 4.2 `/vault unseal` — enter masked unseal key input mode, show progress (n/threshold), repeat until unsealed or cancelled
- [x] 4.3 `/vault configure` — set Vault address, auth method, persist to vault.json
- [x] 4.4 `/vault login` — interactive token/AppRole credential entry, store in keyring
- [x] 4.5 `/vault init-policy` — generate starter HCL policy file to stdout or a file path
- [x] 4.6 Masked input mode — key entry not echoed, not logged, not stored in history. Separate from normal editor input.
- [x] 4.7 System notification on startup if Vault is configured but sealed

## 5. whoami integration (core/crates/omegon/src/tools/whoami.rs)

- [x] 5.1 Add Vault section — status (active/sealed/unreachable), address, auth method, token TTL remaining
- [x] 5.2 Graceful skip if Vault not configured

## 6. Startup context (core/crates/omegon/src/features/lifecycle.rs)

- [x] 6.1 Check Vault health on session_start, emit SystemNotification if sealed
- [x] 6.2 Include Vault status in dashboard footer data if configured

## Cross-cutting constraints

- Agent must NEVER see unseal keys or root tokens — TUI-only
- Vault client degrades gracefully — None on unreachable, no crash
- Unseal key input masked — not echoed, not logged, not in session history
- Resolved secrets flow through existing redaction pipeline
- Config in ~/.omegon/vault.json — never in recipe file or session state
- Vault policy is primary enforcement — omegon never bypasses server ACLs
- Client-side allowlist is defense-in-depth — checked before every API call
- Cleave child tokens: narrower policy, shorter TTL, optional use-limit
- /vault init-policy generates HCL for operator review — omegon never writes policies to Vault
