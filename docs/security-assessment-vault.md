+++
id = "f5274add-23a2-49c4-a24e-866e916fe681"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Security Assessment: Vault Secret Backend

**Date:** 2026-03-20
**Scope:** `core/crates/omegon-secrets/` — vault.rs, resolve.rs, guards.rs, lib.rs
**Method:** 4-way parallel cleave assessment + manual deep review + fail-closed hardening

## Phase 1: Vulnerability Discovery

### Finding 1 — CRITICAL: Path Traversal Bypasses Allowlist

**File:** `vault.rs` — `check_path_allowed()` + `read()`/`write()`/`list()`

A path like `secret/data/../../sys/seal-status` matches the glob `secret/data/*`
but after `Url::join()` resolves `..` segments, the HTTP request hits `v1/sys/seal-status`.

**Fix:** Reject paths with `..` segments, null bytes, and `%2e%2e` before glob matching.

### Finding 2 — CRITICAL: Empty Allowlist Permits All Paths (Fail-Open)

**File:** `vault.rs` — `check_path_allowed()`

The check `if !self.allowed_paths.is_empty() && ...` skips enforcement when the allowlist
is empty. An operator setting `allowed_paths: []` expected maximum restriction but got zero.

**Fix:** `PathPolicy` enum — `DenyAll` when empty, `AllowList { allow, deny }` when populated.

### Finding 3 — HIGH: Redirect Following Leaks X-Vault-Token

**File:** `vault.rs` — reqwest client builder

Reqwest defaults to following redirects with all headers. A `301` to an attacker-controlled
host would exfiltrate the Vault token.

**Fix:** `redirect(reqwest::redirect::Policy::none())`

### Finding 4 — MEDIUM: Half-Initialized Client Stored on Auth Failure

**File:** `lib.rs` — `init_vault()`

Failed auth stored the client as `Some(client)` — downstream `is_some()` checks treated
Vault as "ready" when every request would fail.

**Fix:** Only store `Some(client)` after successful `authenticate()`. Add `vault_health_probe()`
for unauthenticated health checks.

### Finding 5 — MEDIUM: VAULT_ADDR Injects Hardcoded Allowlist

**File:** `vault.rs` — `load_config()`

VAULT_ADDR-only config silently injected `secret/data/*` — invisible policy the operator
never declared.

**Fix:** VAULT_ADDR-only → `DenyAll` + warning to create `vault.json`.

### Finding 6 — MEDIUM: No URL Scheme Validation

**File:** `vault.rs` — `VaultClient::new()`

Accepted `file://`, `ftp://`, etc.

**Fix:** Validate scheme is `http` or `https`.

### Finding 7 — LOW: Raw Error Bodies in Messages

**File:** `vault.rs` — multiple error paths

`response.text().await.unwrap_or_default()` propagated raw Vault response bodies.

**Fix:** `sanitize_error_body()` — truncate to 200 chars, strip token patterns (`hvs.*`, long base64).

### Finding 8 — LOW: No Connect Timeout

**Fix:** Added `.connect_timeout(Duration::from_secs(10))`.

## Phase 2: Fail-Closed Hardening

All findings addressed through the `vault-fail-closed` OpenSpec change:

| Component | Before | After |
|-----------|--------|-------|
| Empty allowlist | Allow all paths | **Deny all paths** |
| Path traversal | Glob matching only | **Reject `..` before glob** |
| VAULT_ADDR-only | Hardcoded `secret/data/*` | **DenyAll + warning** |
| Auth failure | Client stored (half-init) | **Client = None** |
| Redirects | Follow up to 10 | **Never follow** |
| URL scheme | Any | **http/https only** |
| Error bodies | Raw server text | **Sanitized + truncated** |
| Recipe paths | No validation | **Reject traversal, null, control chars** |
| Recipe keys | No validation | **Reject empty, path separators** |

## Test Coverage

**48 tests** in `omegon-secrets` crate (was 33 before assessment):

| Category | Tests |
|----------|-------|
| Vault HTTP client (health, seal, read, write, list, token, auth) | 14 |
| Path enforcement (allowlist, denylist, traversal, DenyAll) | 10 |
| Network hardening (redirect, scheme validation) | 2 |
| Error sanitization | 1 |
| Fail-closed (empty allowlist, DenyAll + health, PathPolicy variants) | 6 |
| Recipe validation (traversal, empty key, path sep, null byte) | 5 |
| Guards (vault.json blocked, .vault-token blocked) | 4 |
| Redaction + resolution | 6 |

## Accepted Risks

1. **`mint_child_token()` returns `String`** — flows directly to cleave dispatch, not logged
2. **No response size limit** — Vault is operator-controlled infrastructure
3. **DNS rebinding** — mitigated by no-redirect policy; full mitigation would require per-request DNS pinning

## Verification

```bash
cd core && cargo test -p omegon-secrets --lib
# 48 passed, 0 failed
```
