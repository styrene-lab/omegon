# Styrene Identity as operator credential root — RNS identity for secret unlocking and trust — Design

## Architecture Decisions

### Decision: Separate secrets.db, encrypted at rest, never in git

**Status:** decided
**Rationale:** Different threat model (credentials vs knowledge), different lifecycle (never synced/archived casually), different encryption boundary (secrets.db encrypted at rest, memory.db plaintext for semantic search). Located at ~/.config/omegon/secrets.db. Never appears in any git repo, backup, or sync mechanism without explicit operator action.

### Decision: Mesh secrets are live lookups, no caching

**Status:** decided
**Rationale:** Caching introduces invalidation, staleness, and a larger encrypted attack surface for zero demonstrated need. If the mesh is down, mesh secrets are unavailable — same as Vault when unreachable. Local secrets (keyring, passphrase-encrypted, env vars) are the offline path. Mesh secrets are by definition online resources.

### Decision: Three encryption backends: Styrene Identity (feature-gated), OS keyring (default), passphrase with Argon2id (default)

**Status:** decided
**Rationale:** Dropped age crate — it solves a niche case that passphrase encryption handles without a new dependency. (1) Styrene Identity: HKDF-derived AES key from RNS Ed25519/X25519 keypair, feature-gated with --features=styrene. (2) OS keyring: platform credential store via keyring crate, default for desktop operators. (3) Passphrase: AES-256-GCM with Argon2id key derivation, default for headless servers with no keyring daemon. Uses aes-gcm already in the dependency tree via styrene-tunnel. All three produce the same encryption key for secrets.db — the operator picks during `omegon secrets init`.

## Research Context

### What a Styrene Identity provides

A Styrene `PrivateIdentity` (styrene-rns crate) holds:
- **Ed25519 SigningKey** — digital signatures, message authentication
- **X25519 StaticSecret** — ECDH key agreement, derived shared secrets
- **AddressHash** — unique mesh address derived from the public keys
- **DerivedKey** — HKDF-derived symmetric key from ECDH exchange
- **Fernet encrypt/decrypt** — symmetric encryption using the derived key

The Identity is the operator's mesh persona — unique, cryptographically bound, and portable. It's generated once (`PrivateIdentity::new_from_rand()`) and stored locally.

This gives us three potential uses in Omegon:

**1. Secret store encryption key**
The operator's RNS private key derives an encryption key (via HKDF) that encrypts the local secret store. No Styrene Identity → no access to secrets. This replaces (or supplements) the OS keyring as the root of trust.

**2. Trust anchor for remote MCP servers**
When connecting to an MCP server on a remote Styrene node, both sides already have RNS identities. The PQC tunnel handshake authenticates both endpoints. Omegon can verify that the remote MCP server is running on a *specific* node it trusts, not just any node that responds.

**3. Operator authentication across Omegon instances**
If the same operator runs Omegon on multiple machines, their Styrene Identity ties those instances together. Memory facts, persona preferences, and secrets can be encrypted to the operator's identity and synced over the mesh.

### How it layers with existing secret backends

Today omegon-secrets resolves credentials through a fallback chain:
1. **Environment variables** — `ANTHROPIC_API_KEY` etc.
2. **Shell commands** — `$(op read "op://vault/key")` for 1Password etc.
3. **OS keyring** — `keyring` crate (macOS Keychain, Linux Secret Service, Windows Credential Manager)
4. **Vault** — HashiCorp Vault with token/AppRole/K8s SA auth, PathPolicy enforcement

The question: where does Styrene Identity fit in this chain?

**Option A: Styrene as global unlock (replaces OS keyring)**
The operator's RNS identity derives an AES key that encrypts a local secrets database. The OS keyring is no longer the root of trust — the Styrene Identity is. This is stronger (cryptographic, portable, identity-bound) but requires the Styrene daemon to be running.

**Option B: Styrene as additional backend (alongside OS keyring)**
Styrene Identity is a new resolution method: `styrene://` URIs in recipes resolve by asking the Styrene daemon, which may fetch from mesh-accessible secret stores on other nodes. The OS keyring remains the local fallback. This is more flexible — operators without Styrene get the same experience.

**Option C: Styrene as Vault auth method (bridge)**
The operator's Styrene Identity authenticates to Vault via a custom auth plugin. Vault remains the secret store, Styrene provides the identity assertion. This keeps the existing Vault infrastructure but adds a stronger auth method.

**Recommendation: Option B — additive, not replacing.**
- Operators without Styrene: env vars + keyring + Vault (unchanged)
- Operators with Styrene: all of the above + identity-encrypted local store + mesh-accessible secrets
- The Styrene Identity *can* be the only credential source for an operator who wants it, but it's not required

This follows the Lex Imperialis principle of operator agency — the operator chooses their trust model.

### FOSS alternative consideration — self-sovereign identity without Styrene

Not every operator will have Styrene. We need a FOSS identity-based secret encryption path that doesn't depend on the mesh:

**age (filippo.io/age)** — modern, simple file encryption. The operator generates an age identity (`age-keygen`), uses it to encrypt the local secret store. The age crate is pure Rust, widely adopted. This gives identity-bound encryption without any daemon.

**SSH keys** — the operator already has SSH keys. `age` supports encrypting to SSH keys natively. No new key material to manage.

**Proposed layered model:**
```
Secret resolution chain:
  1. Environment variables (always)
  2. Shell commands (1Password, etc.)
  3. Local encrypted store:
     a. Styrene Identity (if daemon available) — AES via HKDF from RNS key
     b. age Identity (if ~/.config/omegon/identity.txt exists) — age encryption
     c. OS keyring (fallback)
  4. Vault (if configured)
  5. Mesh-accessible secrets via Styrene (if connected)
```

The encrypted store uses whoever's available. `omegon secrets init` creates the identity:
- If Styrene daemon is running: uses the RNS identity
- Otherwise: generates an age identity
- Operator can bring their own SSH key: `omegon secrets init --ssh-key ~/.ssh/id_ed25519`

The bootstrap flow (from the native-local-inference design) should show the secret backend status:
```
  Secrets:
    ✓ Styrene Identity    a7b3c9d1...    (RNS Ed25519)
    ✓ OS Keyring          macOS Keychain  (3 stored keys)
    ✓ Vault               vault.local:8200 (AppRole auth)
    ○ age Identity        not configured
```
