+++
id = "a2ca6066-d87d-4058-8d24-b8d86e092303"
kind = "document"
title = "Vault client fail-closed security hardening"
status = "implemented"
tags = ["security", "vault"]
aliases = ["vault-fail-closed"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "vault-fail-closed"
parent = "vault-secret-backend"
+++

# Vault client fail-closed security hardening

## Overview

Security assessment revealed multiple fail-open defaults in the vault client. The client-side path enforcement uses a deny-list-first model where an empty allowlist permits all paths. Auth failures leave a "configured but unauthenticated" client that downstream code treats as ready. Error paths propagate raw server response bodies. All of these must flip to fail-closed: deny by default, reject ambiguity, surface failures loudly.

## Decisions

### Decision: Empty allowlist must deny all paths, not allow all

**Status:** decided
**Rationale:** The current check `if !self.allowed_paths.is_empty() && !self.allowed_paths.is_match(path)` skips enforcement entirely when no patterns are configured. An operator who sets `allowed_paths: []` in vault.json expects maximum restriction — instead they get zero restriction. The fix is a PathPolicy enum that makes the deny-by-default explicit: DenyAll when empty, AllowList when populated. The deny list is always checked regardless.

### Decision: Auth failure must not store a half-initialized client

**Status:** decided
**Rationale:** Currently init_vault() stores the VaultClient even when authentication fails. Downstream code checks `vault_client.is_some()` to decide whether Vault is "configured" — leading to confusing states where the client exists but every request fails with "no token". A fail-closed init either succeeds fully (authed client) or results in None with a clear error. Health-only checks (seal status, whoami) get a separate unauthenticated path.

### Decision: VAULT_ADDR without vault.json must require explicit operator confirmation of path scope

**Status:** decided
**Rationale:** When only VAULT_ADDR is set, the current code injects a hardcoded allowlist of secret/data/*. This is a reasonable default but it's invisible policy. The operator set an address but never declared which paths are safe. Fail-closed behavior: VAULT_ADDR-only config should start with DenyAll and emit a startup warning telling the operator to create vault.json with explicit allowed_paths. Health checks still work (they don't go through path enforcement).

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon-secrets/src/vault.rs` (modified) — Add PathPolicy enum, refactor check_path_allowed to fail-closed, add addr validation, redirect policy, connect timeout, error body sanitization
- `core/crates/omegon-secrets/src/lib.rs` (modified) — Refactor init_vault to fail-closed auth — only store authenticated client, separate health-only probe
- `core/crates/omegon-secrets/src/resolve.rs` (modified) — Validate vault: recipe path segments before passing to VaultClient

### Constraints

- Every code path that can touch secrets must default to deny on unexpected conditions
- Empty allowlist = deny all, not allow all
- Auth failure = no client stored, not half-initialized client
- Error messages must not include raw Vault response bodies
- VAULT_ADDR-only must start with DenyAll + startup warning
