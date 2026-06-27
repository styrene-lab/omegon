# Web UI Backend RBAC and Identity Requirements

This document captures the backend requirements needed to fully support the Omegon Web UI and Auspex-style trusted proxy deployments.

## Scope

Applies to:

- Native Web UI HTTP APIs documented in `docs/web-api.openapi.yaml`.
- Legacy compatibility WebSocket `/ws` documented in `docs/web-ws-contract.md`.
- Surface snapshot and stream endpoints.
- Daemon event ingress.
- Local bearer-token access and trusted proxy principal headers.

Out of scope:

- Frontend layout and rendering behavior.
- Non-Web UI provider behavior.
- Long-term fine-grained grants beyond the current Styrene role mapping.

## Identity requirements

### Principal model

The backend must resolve each protected request to a `WebPrincipal` before authorization.

A principal contains:

- `subject`: stable identity string.
- `display_name`: optional human-readable name.
- `issuer`: principal source, such as local token or trusted proxy.
- `auth_source`: auth mechanism/source metadata.
- `role`: Styrene role used for RBAC decisions.
- `session_id`: optional upstream session/audit id.
- `client_id`: optional upstream client/audit id.

### Local bearer mode

When no trusted proxy marker is present:

- The request must carry a valid web-auth bearer token where the route requires header auth.
- The principal is local:
  - `subject = local-web`
  - `issuer = LocalToken`
  - `role = configured WebState role`
- Stray principal role/subject headers must not override local identity.

### Trusted proxy mode

When a trusted proxy marker is present:

- The bearer token must still be valid.
- The proxy issuer must be trusted by backend policy.
- Required principal headers:
  - `Omegon-Principal-Issuer`
  - `Omegon-Principal-Subject`
  - `Omegon-Principal-Role`
- Optional audit headers:
  - `Omegon-Principal-Display-Name`
  - `Omegon-Principal-Session-Id`
  - `Omegon-Principal-Client-Id`
- Untrusted issuer, missing subject, missing role, or invalid role must be rejected before route execution.

Current trusted issuer policy is static and accepts `auspex`. If additional proxies are expected, this must become config-driven.

## RBAC requirements

### Operation enforcement

Every protected backend route must enforce its declared `OmegonOperation` before reading sensitive state or mutating runtime state.

Required operation mapping:

| Surface | Route / message | Operation / classification |
|---|---|---|
| Native session create | `POST /api/sessions` | `native_session.create` |
| Native session read | `GET /api/sessions/{session_id}` | `native_session.read` |
| Surface snapshot | `GET /api/sessions/{session_id}/surfaces` | `surface.read` |
| Legacy surface snapshot | `GET /api/web/surfaces` | `surface.read` |
| Surface stream | `/api/sessions/{session_id}/surfaces/stream` | `surface.stream` |
| Legacy surface stream | `/api/web/surfaces/stream` | `surface.stream` |
| Native action ingress | `POST /api/sessions/{session_id}/actions` | `native_session.action` |
| Legacy action ingress | `POST /api/web/actions` | `native_session.action` |
| Daemon event ingress | `POST /api/events` | `event.ingress` plus trigger-specific role check |
| Legacy `/ws` messages | `/ws` JSON command frames | `control_actions.rs` classification |

### Role behavior

Minimum expected behavior:

- Monitor/read role:
  - may read sessions and surfaces.
  - may stream surfaces.
  - may not create sessions, submit prompts/actions, or ingress daemon events requiring edit/admin.
- Operator/edit role:
  - may perform edit-level Web UI operations, including prompt/action submission.
  - may not perform admin-only operations.
- Admin role:
  - may perform admin-level operations.
- Blocked/none roles:
  - must be denied for protected APIs.

### Caller role assertions

For daemon event ingress:

- `caller_role` must be present.
- `caller_role` must parse to a known role.
- `caller_role` must match the resolved principal role.
- Request bodies must not be able to self-escalate role.

For legacy `/ws`:

- `caller_role` is optional.
- Omitted role uses the configured connection principal role.
- Unknown labels degrade to read, not admin.
- Per-message authorization still gates command execution.

## Authentication requirements

### HTTP APIs

Protected native/action/event APIs must require bearer auth and, when supplied, validate trusted proxy principal headers.

### Streams

Surface stream endpoints must support both:

1. Query-token compatibility for browser WebSocket clients.
2. Bearer/principal header path for proxy-capable clients.

Both paths must enforce `surface.stream` before upgrade.

### Legacy `/ws`

Legacy `/ws` requires `?token=<web-auth-token>` at upgrade and must reject missing/invalid token with `401` before upgrade.

## Contract requirements

Authoritative contracts:

- HTTP/native APIs: `docs/web-api.openapi.yaml`
- Legacy WebSocket control protocol: `docs/web-ws-contract.md`

Contract docs must stay aligned with:

- route availability,
- auth requirements,
- principal headers,
- RBAC failure responses,
- WebSocket message names and role classifications.

Behavior changes affecting Web UI backend, auth, identity, RBAC, or operator workflow must update `CHANGELOG.md`.

## Validation requirements

Before considering this backend surface clean:

- `cargo test -p omegon` must pass in normal parallel mode.
- `cargo test -p omegon --test openapi_contract_lint` must pass.
- Focused Web/RBAC tests should cover:
  - monitor denial for mutation routes,
  - operator/admin allowance for mutation routes,
  - blocked denial for read/stream routes,
  - event ingress missing/mismatched role rejection,
  - trusted proxy principal acceptance/rejection,
  - legacy `/ws` prompt/cancel denial for read role.
- `git diff --check` must pass before commit.
- `just link` should be run after backend changes when preparing the local development binary.

## Remaining recommended work

1. End-to-end Web UI/Auspex smoke test against the committed contract.
2. Make trusted proxy issuers config-driven if deployments beyond Auspex are needed.
3. Add a lightweight repeatable smoke script/test for native HTTP + stream + `/ws` behavior.
4. Keep frontend/proxy implementation aligned to the standardized `Omegon-Principal-*` headers.
