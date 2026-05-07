+++
id = "caa57633-6245-4da1-8e5f-21d29581dda6"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vault client fail-closed security hardening — Design

## Architecture Decisions

### Decision: Empty allowlist must deny all paths, not allow all

**Status:** decided
**Rationale:** The current check `if !self.allowed_paths.is_empty() && !self.allowed_paths.is_match(path)` skips enforcement entirely when no patterns are configured. An operator who sets `allowed_paths: []` in vault.json expects maximum restriction — instead they get zero restriction. The fix is a PathPolicy enum that makes the deny-by-default explicit: DenyAll when empty, AllowList when populated. The deny list is always checked regardless.

### Decision: Auth failure must not store a half-initialized client

**Status:** decided
**Rationale:** Currently init_vault() stores the VaultClient even when authentication fails. Downstream code checks `vault_client.is_some()` to decide whether Vault is "configured" — leading to confusing states where the client exists but every request fails with "no token". A fail-closed init either succeeds fully (authed client) or results in None with a clear error. Health-only checks (seal status, whoami) get a separate unauthenticated path.

### Decision: VAULT_ADDR without vault.json must require explicit operator confirmation of path scope

**Status:** decided
**Rationale:** When only VAULT_ADDR is set, the current code injects a hardcoded allowlist of secret/data/*. This is a reasonable default but it's invisible policy. The operator set an address but never declared which paths are safe. Fail-closed behavior: VAULT_ADDR-only config should start with DenyAll and emit a startup warning telling the operator to create vault.json with explicit allowed_paths. Health checks still work (they don't go through path enforcement).

## File Changes

- `core/crates/omegon-secrets/src/vault.rs` (modified) — Add PathPolicy enum, refactor check_path_allowed to fail-closed, add addr validation, redirect policy, connect timeout, error body sanitization
- `core/crates/omegon-secrets/src/lib.rs` (modified) — Refactor init_vault to fail-closed auth — only store authenticated client, separate health-only probe
- `core/crates/omegon-secrets/src/resolve.rs` (modified) — Validate vault: recipe path segments before passing to VaultClient

## Constraints

- Every code path that can touch secrets must default to deny on unexpected conditions
- Empty allowlist = deny all, not allow all
- Auth failure = no client stored, not half-initialized client
- Error messages must not include raw Vault response bodies
- VAULT_ADDR-only must start with DenyAll + startup warning
