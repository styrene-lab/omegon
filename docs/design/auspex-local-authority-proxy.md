# Auspex Local Authority Proxy — Omegon Additions

Status: draft for Omegon implementation
Owner: Omegon daemon/web control-plane
Related Auspex commits: `621f5b3`, `452ffa2`, `b0b8f6a`, `4d6a0a8`, `bc911df`

## Problem

Auspex now runs the browser UI behind a local authority proxy instead of sending the browser directly to the Omegon daemon. The proxy owns daemon bearer discovery, injects trusted local principal headers, and bridges the surface WebSocket stream.

The current daemon still treats the bearer token as the main authority. That is acceptable for localhost MVP, but it does not let Omegon distinguish:

1. a browser/client that directly obtained or copied the bearer token, from
2. the Auspex local authority proxy asserting a single local Styrene/operator identity.

Omegon needs a small strict-mode trust layer so deployments can opt into: "only this local proxy identity may assert web principals."

This document lays out the daemon-side additions needed in `../omegon-secundus`.

## Current Auspex behavior

The Auspex proxy is a native helper at `auspex/src/bin/omegon_web_proxy.rs`.

It currently:

- listens on `127.0.0.1:9311`
- forwards `/api/*` to Omegon, normally `http://127.0.0.1:8080/api/*`
- discovers the current Omegon token from `/api/startup`
- injects `Authorization: Bearer <current token>` for HTTP requests
- rewrites upstream WebSocket surface stream URLs as `...?token=<current token>`
- strips browser-supplied spoofable authority headers before forwarding
- injects local principal headers:
  - `Omegon-Principal-Issuer: auspex`
  - `Omegon-Principal-Subject: styrene:local-operator:<uuid>` when configured
  - `Omegon-Principal-Role: operator`
  - `Omegon-Principal-Client-Id: auspex-web`
  - `Omegon-Back-Url: http://127.0.0.1:9310/`
  - `Auspex-Proxy-Identity-Fingerprint: <fingerprint>` when configured
- exposes status at `/_auspex/proxy/status`

Example status after `--init-identity`:

```json
{
  "schema_version": 1,
  "mode": "proxy-mediated",
  "browser_tls": { "enabled": false, "trusted_local_ca": false },
  "daemon": {
    "base_url": "http://127.0.0.1:8080",
    "reachable": true,
    "token_cached": false
  },
  "identity": {
    "configured": true,
    "subject": "styrene:local-operator:b59adc98-12d3-4428-a224-dd4b2791fec3",
    "fingerprint": "07be5a751c1f4d2ca2fa8f1ee129c458",
    "strict_daemon_identity": false
  },
  "websocket": { "surface_stream_proxy": true }
}
```

The current identity artifact is intentionally simple:

```json
{
  "schema_version": 1,
  "subject": "styrene:local-operator:<uuid>",
  "fingerprint": "<stable local fingerprint>",
  "created_at_unix": 1783024900,
  "strict_daemon_identity": false
}
```

This is not yet cryptographic proof. It is a stable local authority identifier and a contract for future stronger proof.

## Required daemon behavior

### 1. Add a web local-authority trust config

Add daemon configuration for a trusted local web proxy identity.

Suggested CLI/options:

```text
omegon serve \
  --web-trusted-proxy-identity .auspex/identity/web-proxy.json \
  --require-web-proxy-identity
```

Alternative environment names are acceptable, but the behavior should be explicit:

- `--web-trusted-proxy-identity <path>` loads a local authority identity document.
- `--require-web-proxy-identity` enables strict mode.
- Without strict mode, current bearer behavior remains compatible.

Suggested data model:

```rust
pub struct WebTrustedProxyIdentity {
    pub schema_version: u8,
    pub subject: String,
    pub fingerprint: String,
    pub strict_daemon_identity: bool,
}
```

The file format should intentionally match Auspex's current JSON so both projects can share it.

### 2. Centralize proxy identity header parsing

Add constants and parser helpers near the existing web principal/RBAC code.

Current likely files:

- `core/crates/omegon/src/web/rbac.rs`
- `core/crates/omegon/src/web/surface_stream.rs`
- `core/crates/omegon/src/web/auth.rs`
- `core/crates/omegon/src/web/mod.rs`

Headers to recognize:

```text
Omegon-Principal-Issuer
Omegon-Principal-Subject
Omegon-Principal-Role
Omegon-Principal-Display-Name
Omegon-Principal-Session-Id
Omegon-Principal-Client-Id
Omegon-Back-Url
Auspex-Proxy-Identity-Fingerprint
```

Add a helper like:

```rust
pub struct WebProxyIdentityAssertion {
    pub issuer: String,
    pub subject: String,
    pub role: styrene_rbac::Role,
    pub client_id: Option<String>,
    pub back_url: Option<String>,
    pub fingerprint: Option<String>,
}

pub fn proxy_identity_assertion_from_headers(headers: &HeaderMap)
    -> Option<WebProxyIdentityAssertion>;
```

### 3. Validate proxy identity assertions in strict mode

When `--require-web-proxy-identity` is enabled:

- A valid bearer token is still required for now.
- Principal headers are accepted only if:
  - `Omegon-Principal-Issuer == "auspex"`
  - `Omegon-Principal-Subject == trusted_identity.subject`
  - `Auspex-Proxy-Identity-Fingerprint == trusted_identity.fingerprint`
- Missing/mismatched identity returns `401` or `403` with a stable error string.

Suggested error strings:

- `missing_proxy_identity`
- `proxy_identity_mismatch`
- `proxy_identity_required`

This keeps the rollout safe: the bearer still gates daemon access, but strict mode also requires the local proxy identity assertion.

### 4. Apply validation consistently across HTTP and WebSocket surfaces

The strict identity check needs to apply to:

- `GET /api/sessions/{id}/surfaces`
- `POST /api/sessions/{id}/actions`
- `GET /api/sessions/{id}/surfaces/stream` WebSocket upgrade
- any future RBAC-gated `/api/sessions/*` web routes

Important WebSocket note:

The Auspex proxy currently authenticates the upstream WebSocket by appending the daemon bearer as `?token=<current token>`. Browser clients cannot set arbitrary WebSocket headers, but the proxy can. If Omegon strict mode requires identity for WebSocket upgrades, Auspex can be updated to send identity headers during the upstream `tokio_tungstenite` handshake.

Daemon should support identity validation from WebSocket upgrade headers, not only query params.

### 5. Avoid trusting principal headers without the configured local authority

In non-strict compatibility mode, current behavior can remain. In strict mode:

- Do not allow arbitrary `Omegon-Principal-*` headers to elevate role.
- Do not treat `Omegon-Principal-Role: admin` as authoritative unless the proxy identity also matches the configured trusted identity.
- Prefer the configured default web role when identity assertion is absent.

This prevents any localhost process with a token from spoofing higher authority using browser-visible headers.

### 6. Expose status in startup/state

Add enough data for Auspex UI diagnostics.

Suggested addition to `/api/startup` or `/api/state`:

```json
"web_authority": {
  "mode": "bearer" | "trusted_proxy" | "trusted_proxy_strict",
  "trusted_proxy_configured": true,
  "trusted_proxy_subject": "styrene:local-operator:...",
  "trusted_proxy_fingerprint": "...",
  "strict_proxy_identity": true
}
```

Do not include private material.

In strict mode, consider omitting the bearer token from public startup responses unless the request arrives over an already trusted local channel. For MVP, it is acceptable to leave this as a follow-up as long as strict mode validates proxy identity on privileged routes.

## Acceptance criteria

### Compatibility mode

Given Omegon starts without `--require-web-proxy-identity`
When Auspex proxy forwards bearer-authenticated requests
Then existing `/api/sessions/default/surfaces`, actions, and stream continue to work.

### Strict mode accepts configured proxy identity

Given Omegon starts with:

```text
--web-trusted-proxy-identity .auspex/identity/web-proxy.json
--require-web-proxy-identity
```

And the request carries:

```text
Authorization: Bearer <valid>
Omegon-Principal-Issuer: auspex
Omegon-Principal-Subject: <trusted subject>
Auspex-Proxy-Identity-Fingerprint: <trusted fingerprint>
```

When the proxy calls `/api/sessions/default/surfaces`
Then Omegon returns `200`.

### Strict mode rejects missing identity

Given strict mode is enabled
And the request carries only a valid bearer token
When the request calls `/api/sessions/default/surfaces`
Then Omegon rejects it with `401` or `403`
And the body/error contains `proxy_identity_required` or `missing_proxy_identity`.

### Strict mode rejects mismatched identity

Given strict mode is enabled
And the request carries a valid bearer token
And `Auspex-Proxy-Identity-Fingerprint` does not match the configured file
When the request calls `/api/sessions/default/actions`
Then Omegon rejects it with `401` or `403`
And the body/error contains `proxy_identity_mismatch`.

### WebSocket stream respects strict identity

Given strict mode is enabled
And the upstream WebSocket upgrade carries the valid query token and proxy identity headers
When the proxy connects to `/api/sessions/default/surfaces/stream`
Then Omegon accepts the stream and sends an initial snapshot envelope.

Given the same request without matching identity headers
Then the upgrade is rejected.

## Suggested tests

Add unit tests around `web/rbac.rs` and `web/surface_stream.rs` similar to existing bearer/principal tests.

Candidate test names:

```rust
principal_from_headers_accepts_trusted_proxy_identity_in_strict_mode
principal_from_headers_rejects_missing_proxy_identity_in_strict_mode
principal_from_headers_rejects_proxy_identity_fingerprint_mismatch
web_action_rejects_spoofed_principal_without_trusted_proxy_identity
surface_stream_accepts_trusted_proxy_identity_headers
surface_stream_rejects_query_token_only_when_proxy_identity_required
startup_reports_trusted_proxy_authority_status
```

## Implementation notes

- Keep the first implementation file-backed and local-only. Do not design a network PKI yet.
- Treat the current `fingerprint` as a shared local binding value, not a cryptographic signature.
- The next hardening step can replace the simple fingerprint with signed challenges or a cert-backed assertion without changing the high-level API shape.
- Header constants should live in one place so Auspex and Omegon do not drift.
- The strict mode should be opt-in; default behavior must keep existing 0.27.0 browser/control integrations working.

## Future hardening, not required for first implementation

- HTTPS listener for the Auspex proxy with generated local CA.
- Browser trust install helper.
- Signed proxy assertions per request:
  - nonce
  - timestamp
  - canonical request target
  - signature with local authority key
- Daemon-side replay window for signed assertions.
- Omit daemon bearer from `/api/startup` when a trusted proxy is configured.
- IPC-only token discovery between proxy and daemon.
