+++
id = "6c0d6c1c-ca70-4074-b19d-843e19b79413"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Styrene Identity as operator credential root — RNS identity for secret unlocking and trust — Design Spec (extracted)

> Auto-extracted from docs/styrene-identity-secrets.md at decide-time.

## Decisions

### Separate secrets.db, encrypted at rest, never in git (decided)

Different threat model (credentials vs knowledge), different lifecycle (never synced/archived casually), different encryption boundary (secrets.db encrypted at rest, memory.db plaintext for semantic search). Located at ~/.config/omegon/secrets.db. Never appears in any git repo, backup, or sync mechanism without explicit operator action.

### Mesh secrets are live lookups, no caching (decided)

Caching introduces invalidation, staleness, and a larger encrypted attack surface for zero demonstrated need. If the mesh is down, mesh secrets are unavailable — same as Vault when unreachable. Local secrets (keyring, passphrase-encrypted, env vars) are the offline path. Mesh secrets are by definition online resources.

### Three encryption backends: Styrene Identity (feature-gated), OS keyring (default), passphrase with Argon2id (default) (decided)

Dropped age crate — it solves a niche case that passphrase encryption handles without a new dependency. (1) Styrene Identity: HKDF-derived AES key from RNS Ed25519/X25519 keypair, feature-gated with --features=styrene. (2) OS keyring: platform credential store via keyring crate, default for desktop operators. (3) Passphrase: AES-256-GCM with Argon2id key derivation, default for headless servers with no keyring daemon. Uses aes-gcm already in the dependency tree via styrene-tunnel. All three produce the same encryption key for secrets.db — the operator picks during `omegon secrets init`.

## Research Summary

### What a Styrene Identity provides

A Styrene `PrivateIdentity` (styrene-rns crate) holds:
- **Ed25519 SigningKey** — digital signatures, message authentication
- **X25519 StaticSecret** — ECDH key agreement, derived shared secrets
- **AddressHash** — unique mesh address derived from the public keys
- **DerivedKey** — HKDF-derived symmetric key from ECDH exchange
- **Fernet encrypt/decrypt** — symmetric encryption using the derived key

The Identity is the operator's mesh persona — unique, cryptographically bound, and portable. It…

### How it layers with existing secret backends

Today omegon-secrets resolves credentials through a fallback chain:
1. **Environment variables** — `ANTHROPIC_API_KEY` etc.
2. **Shell commands** — `$(op read "op://vault/key")` for 1Password etc.
3. **OS keyring** — `keyring` crate (macOS Keychain, Linux Secret Service, Windows Credential Manager)
4. **Vault** — HashiCorp Vault with token/AppRole/K8s SA auth, PathPolicy enforcement

The question: where does Styrene Identity fit in this chain?

**Option A: Styrene as global unlock (replaces OS k…

### FOSS alternative consideration — self-sovereign identity without Styrene

Not every operator will have Styrene. We need a FOSS identity-based secret encryption path that doesn't depend on the mesh:

**age (filippo.io/age)** — modern, simple file encryption. The operator generates an age identity (`age-keygen`), uses it to encrypt the local secret store. The age crate is pure Rust, widely adopted. This gives identity-bound encryption without any daemon.

**SSH keys** — the operator already has SSH keys. `age` supports encrypting to SSH keys natively. No new key materia…
