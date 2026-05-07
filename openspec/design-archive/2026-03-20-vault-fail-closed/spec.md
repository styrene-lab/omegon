+++
id = "1bfdb661-c1ad-4506-b2ea-086a1a075080"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Vault client fail-closed security hardening — Design Spec (extracted)

> Auto-extracted from docs/vault-fail-closed.md at decide-time.

## Decisions

### Empty allowlist must deny all paths, not allow all (decided)

The current check `if !self.allowed_paths.is_empty() && !self.allowed_paths.is_match(path)` skips enforcement entirely when no patterns are configured. An operator who sets `allowed_paths: []` in vault.json expects maximum restriction — instead they get zero restriction. The fix is a PathPolicy enum that makes the deny-by-default explicit: DenyAll when empty, AllowList when populated. The deny list is always checked regardless.

### Auth failure must not store a half-initialized client (decided)

Currently init_vault() stores the VaultClient even when authentication fails. Downstream code checks `vault_client.is_some()` to decide whether Vault is "configured" — leading to confusing states where the client exists but every request fails with "no token". A fail-closed init either succeeds fully (authed client) or results in None with a clear error. Health-only checks (seal status, whoami) get a separate unauthenticated path.

### VAULT_ADDR without vault.json must require explicit operator confirmation of path scope (decided)

When only VAULT_ADDR is set, the current code injects a hardcoded allowlist of secret/data/*. This is a reasonable default but it's invisible policy. The operator set an address but never declared which paths are safe. Fail-closed behavior: VAULT_ADDR-only config should start with DenyAll and emit a startup warning telling the operator to create vault.json with explicit allowed_paths. Health checks still work (they don't go through path enforcement).
