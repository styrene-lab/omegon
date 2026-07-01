+++
id = "3cf24d87-61be-4b10-a780-eb6365883478"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Styrene Identity as operator credential root — RNS identity for secret unlocking and trust — Tasks

## 1. Encrypted secrets.db store
<!-- specs: secrets/store -->

- [x] 1.1 SQLite store at `~/.config/omegon/secrets.db`, WAL mode, never in git (`omegon-secrets/src/store.rs`)
- [x] 1.2 Per-secret AES-256-GCM encryption at rest
- [x] 1.3 Store-level unit tests (13 in store.rs)

## 2. Encryption backends

- [x] 2.1 OS keyring backend (default) — store key via `keyring_set("sh.styrene.omegon", "store-key")`
- [x] 2.2 Passphrase backend — AES key derived via Argon2id (`argon2` 0.5)

## 3. Deferred to post-0.27.0 — Styrene Identity backend

Deferred by operator decision (2026-07-01, release assessment): blocked on the
RNS identity stack being available as a dependency. Not release-gating for
0.27.0; the shipped keyring/passphrase backends are the complete 0.27.0 scope.

- [ ] 3.1 Styrene Identity backend — HKDF-derived key from RNS Ed25519/X25519, behind a `styrene-identity` cargo feature (documented in store.rs module header; no feature flag or implementation exists yet)
- [ ] 3.2 Backend selection/fallback order: identity (if feature + identity present) → keyring → passphrase prompt

## 4. Deferred to post-0.27.0 — Mesh secrets

- [ ] 4.1 Mesh secret lookups resolve live against the RNS mesh — no local caching of mesh-delivered values
- [ ] 4.2 Trust decisions keyed to RNS identity fingerprints

> Implementation note (2026-06-12): Groups 1 and 2 shipped in the
> omegon-secrets crate. The Styrene Identity backend (group 3) and mesh
> lookups (group 4) are blocked on the RNS identity stack being available
> as a dependency — substantial feature work, not bookkeeping. Original
> scaffolder one-liners replaced with the actual task breakdown.
>
> Deferral note (2026-07-01): groups 3 and 4 formally deferred to
> post-0.27.0 during the release assessment (docs/release-0.27.0-assessment.md,
> decision D3). This change does not gate the 0.27.0 cut.
