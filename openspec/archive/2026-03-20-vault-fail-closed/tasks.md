+++
id = "dc52d685-debf-468a-8827-cde7c83d654c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vault client fail-closed security hardening — Tasks

## 1. PathPolicy enum and fail-closed path enforcement (vault.rs)
<!-- specs: vault-security -->

- [x] 1.1 Add `PathPolicy` enum: `DenyAll`, `AllowList { allow: GlobSet, deny: GlobSet }`
- [x] 1.2 `PathPolicy::from_config(allowed, denied)` — empty allowed → DenyAll
- [x] 1.3 Refactor `check_path_allowed()` to delegate to `PathPolicy::check()`
- [x] 1.4 Update `VaultClient::new()` to use `PathPolicy::from_config()`
- [x] 1.5 health(), seal_status(), unseal() bypass path enforcement (fixed endpoints)
- [x] 1.6 Tests: empty allowlist denies, explicit allowlist allows, DenyAll + health still works

## 2. VAULT_ADDR deny-all default and error sanitization (vault.rs)
<!-- specs: vault-security -->

- [x] 2.1 VAULT_ADDR fallback uses `allowed_paths: vec![]` (DenyAll)
- [x] 2.2 tracing::warn when VAULT_ADDR-only: "create vault.json with allowed_paths"
- [x] 2.3 Add `sanitize_error_body()` — truncate 200 chars, strip token patterns
- [x] 2.4 Replace all raw `response.text().await.unwrap_or_default()` with sanitized version
- [x] 2.5 Tests: error body sanitization strips tokens, truncates

## 3. Fail-closed auth in SecretsManager (lib.rs)
<!-- specs: vault-security -->

- [x] 3.1 `init_vault()` only stores `Some(client)` when `authenticate()` succeeds
- [x] 3.2 Add `vault_health_probe()` for unauthenticated health/seal checks
- [x] 3.3 Auth failure logs warning, vault_client remains None

## 4. Recipe path validation (resolve.rs)
<!-- specs: vault-security -->

- [x] 4.1 Validate path before VaultClient: reject `..`, null bytes, control chars
- [x] 4.2 Validate key: reject empty, path separators
- [x] 4.3 Move validation before `vault_client?` so it runs even without client
- [x] 4.4 Tests: 5 recipe validation tests (traversal, empty key, path sep, empty path, null byte)

## Cross-cutting constraints

- [x] Every code path that can touch secrets defaults to deny on unexpected conditions
- [x] Empty allowlist = deny all, not allow all
- [x] Auth failure = no client stored, not half-initialized client
- [x] Error messages do not include raw Vault response bodies
- [x] VAULT_ADDR-only starts with DenyAll + startup warning
