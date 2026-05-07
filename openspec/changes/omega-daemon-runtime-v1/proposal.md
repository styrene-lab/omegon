+++
id = "acd08f02-f42c-4903-a52f-714418958cea"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Omega daemon runtime v1

## Intent

Define and implement the first persistent Omegon/Omega daemon mode with attachable long-lived sessions, minimal managed-instance lifecycle, and a typed event-ingress surface for future webhooks/connectors.

## Scope

Daemon runtime v1 covers:

- a persistent `serve` / daemon mode
- stable instance identity and runtime directory
- a single long-lived server process with one active session plus queued work
- native IPC as the canonical local control plane
- a minimal localhost HTTP surface for health/readiness and typed event ingress
- a typed daemon event envelope suitable for future webhook/connector/scheduler producers
- ownership metadata distinguishing Auspex-managed versus standalone service instances
- explicit transport-security posture for insecure bootstrap listeners versus preferred secure transports

Out of scope for v1:

- multi-instance management inside a single daemon process
- first-class connector runtimes
- first-class webhook product features
- remote fleet orchestration
- Styrene RPC implementation itself

## Constraints

- Plain HTTP and raw WebSocket may exist only as degraded/bootstrap transports and must be warned, not normalized as the happy path.
- HTTPS and WSS are the preferred secure network transports before Styrene RPC lands.
- Local first-party control remains anchored on native IPC semantics.
- The runtime/event contract must remain compatible with future Styrene identity-based RPC and mutual-auth semantics.
