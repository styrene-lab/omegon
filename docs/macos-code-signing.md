---
id: macos-code-signing
title: macOS code signing — stable identity for keychain ACL persistence and Gatekeeper
status: implemented
parent: supply-chain-security
tags: [security, macos, signing, keychain, distribution, apple-developer]
open_questions: []
jj_change_id: movvqpqopmotpqrnsusmmpupowolvmmm
---

# macOS code signing — stable identity for keychain ACL persistence and Gatekeeper

## Overview

macOS Keychain grants 'Always Allow' based on binary CDHash. Unsigned or ad-hoc signed binaries get a new CDHash every build, so operators must re-authorize keychain access on every RC. This requires three tiers of signing: (1) local dev self-signed cert for contributors, (2) Apple Developer ID for public releases, (3) future notarization for Gatekeeper. The signing identity must be stable across builds but distinct from Styrene Identity (which is operator-level, not publisher-level).

## Research

### Three identity layers and how they interact

There are three distinct identity concepts at play:

**1. Apple Developer ID (publisher identity)**
- WHO: Styrene Lab as the software publisher
- WHAT: Apple Developer certificate for code signing + notarization
- WHERE: CI/CD (GitHub Actions) for release builds, local dev with cert export
- WHY: macOS Gatekeeper requires this to run without the "unidentified developer" warning. Keychain ACL persists across builds signed with the same Developer ID.
- COST: $99/year Apple Developer Program
- SCOPE: Signs the binary. This says "Styrene Lab built this."

**2. Sigstore cosign (build provenance)**
- WHO: The CI pipeline (GitHub Actions OIDC identity)
- WHAT: Keyless signatures via Rekor transparency log
- WHERE: CI/CD only (OIDC token only available in Actions)
- WHY: Proves the binary was built from THIS repo by THIS CI pipeline. Complements Apple signing — Apple says "who", Sigstore says "how" and "from what source."
- COST: Free
- SCOPE: Signs the release archive. This says "this binary came from styrene-lab/omegon via GitHub Actions."

**3. Styrene Identity (operator identity)**
- WHO: The individual operator running Omegon
- WHAT: Ed25519/X25519 keypair from the RNS mesh
- WHERE: Operator's machine, encrypted at rest
- WHY: Encrypts the local secrets.db, authenticates to mesh services, ties multiple Omegon instances to one operator
- COST: Free (open source)
- SCOPE: Operator-scoped. This says "I am operator X and these are my secrets."

**Key insight: these are orthogonal.**
- Apple Developer ID = "who published this software" (publisher → user trust)
- Sigstore = "how this binary was built" (build → artifact provenance)
- Styrene Identity = "who is running this software" (operator → secrets trust)

No tie-ins needed between Apple signing and Styrene Identity. They serve different trust domains. An operator with a Styrene Identity running an Apple-signed binary gets both trust assertions, but neither depends on the other.

**However:** the `just setup-signing` flow for local dev contributors DOES need to interact properly with macOS Keychain. The self-signed cert for local dev creates keychain entries that must not collide with the operator's Styrene Identity keychain entries (service name `omegon`). The signing cert goes in the System keychain; the secrets go in the login keychain. No collision.

### Implementation plan — three tiers

**Tier 1: Local dev signing (contributors) — implement now**
- `just setup-signing` creates a self-signed "Omegon Local Dev" certificate
- `just rc` signs the binary with this cert if available, ad-hoc otherwise
- Stable CDHash across builds = macOS Keychain "Always Allow" persists
- No Apple account needed. One-time setup per machine.
- Documented in CONTRIBUTING.md

**Tier 2: Apple Developer ID (public releases) — implement before 0.16.0**
- Apple Developer account already registered
- Create a "Developer ID Application" certificate in the Apple Developer portal
- Export as .p12, store in GitHub Actions secrets
- release.yml signs macOS builds with `codesign -s "Developer ID Application: Styrene Lab"`
- This gives Gatekeeper clearance AND persistent keychain ACL for release users
- install.sh verifies the signature: `codesign -v --verify omegon`

**Tier 3: Notarization (future)**
- Apple notarization = submit the signed binary to Apple for malware scanning
- Returns a "ticket" that macOS checks online
- Required for: `.app` bundles, `.pkg` installers, and soon CLI tools
- `xcrun notarytool submit omegon.zip --apple-id X --password Y --team-id Z`
- Post-notarization: `xcrun stapler staple omegon` (embeds ticket)
- This is the full Gatekeeper experience — no "this app is from an unidentified developer"

**Timeline:**
- Tier 1: this RC (rc.48) — just rc already includes signing step
- Tier 2: before 0.16.0 stable — needs Apple portal cert generation + CI secrets
- Tier 3: before 1.0 — needed for mainstream adoption

### Apple Developer cert as a Styrene Identity credential

The Apple Developer certificate is issued by Apple's CA. You generate a CSR, Apple signs it, you get back a cert + private key (.p12). The cert chains: your cert → Apple WWDR Intermediate → Apple Root CA. macOS trusts it because Apple's root is in the system trust store.

This is fundamentally different from the Styrene Identity's self-sovereign Ed25519 keypair — but it doesn't conflict. They're complementary:

**Styrene Identity as credential wallet:**
The operator's Styrene Identity is the root concept of "me." Platform-specific credentials are attestations that bind TO that identity:

```
Styrene Identity (Ed25519/X25519 — self-sovereign root)
├── Apple Developer ID cert (.p12 — Apple says "this is Styrene Lab")
├── GPG signing key (PGP web of trust)
├── SSH keys (authorized_keys on servers)
├── Sigstore OIDC (GitHub Actions says "built by styrene-lab/omegon")
├── npm publish token (npm trusted publishing)
├── OCI signing key (container image provenance)
└── future: PQC keys (post-quantum mesh transport)
```

Each platform has its own trust chain and issuance process. The Styrene Identity doesn't REPLACE these — it BINDS them. "All of these credentials belong to the same entity."

**Storage model:**
The Apple cert's private key (.p12) is a secret. It should be stored:
- **Local dev:** In the operator's Styrene-encrypted secrets.db (or OS keychain as fallback)
- **CI:** In GitHub Actions encrypted secrets (CI has no Styrene Identity — it's a headless environment)
- **Portable:** The .p12 is exportable, encrypted by the operator's Styrene Identity, and can move between machines

The Styrene Identity doesn't need to "wrap" or "re-sign" the Apple cert. It just provides the encrypted storage and the conceptual binding: "this Apple cert's private key is protected by my Styrene Identity."

**No pre-work needed:**
The Apple cert can be generated, exported, and stored in GitHub Actions secrets TODAY with zero Styrene Identity integration. When the credential wallet feature ships later, the cert can be imported into it retroactively. There's no ordering dependency — generate the cert now, use it in CI now, add it to the identity bundle later.

## Decisions

### Decision: Three signing tiers, no tie-ins to Styrene Identity needed

**Status:** decided
**Rationale:** Apple Developer ID (publisher trust), Sigstore (build provenance), and Styrene Identity (operator trust) are orthogonal. They serve different trust domains and don't share key material. No up-front integration work is needed between Apple signing and Styrene Identity — they can be implemented independently. The only coordination point is macOS Keychain namespace: signing cert → System keychain, operator secrets → login keychain under service name 'omegon'. No collision.

### Decision: Developer ID Application certificate for CLI binary distribution

**Status:** decided
**Rationale:** Developer ID Application is the correct cert type for software distributed outside the Mac App Store. Developer ID Installer is for .pkg installers. We distribute a bare CLI binary via GitHub Releases and install.sh — that's Application, not Installer. If we add a .pkg installer later, we'd generate a second cert.

### Decision: Apple cert in GitHub Actions secrets now, Styrene credential wallet later

**Status:** decided
**Rationale:** CI environments don't have a Styrene Identity — they're headless. The standard pattern (base64-encoded .p12 in APPLE_CERTIFICATE secret, password in APPLE_CERTIFICATE_PASSWORD, Team ID in APPLE_TEAM_ID) works today with no dependencies. When the Styrene Identity credential wallet ships, the cert's private key can be imported retroactively. No ordering dependency — generate and use the cert now, integrate with identity bundle later.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `.github/workflows/release.yml` (modified) — Add macOS signing step: decode APPLE_CERTIFICATE secret, import into build keychain, codesign -s 'Developer ID Application: Styrene Lab' the binary before archiving
- `Justfile` (modified) — just setup-signing creates self-signed cert for local dev. just rc signs with it if available, ad-hoc otherwise. Already implemented in rc.48.
- `CONTRIBUTING.md` (modified) — Document just setup-signing for contributors who hit keychain prompts

### Constraints

- Apple Developer ID Application certificate generated from developer.apple.com portal
- Cert exported as .p12, base64-encoded, stored in GitHub Actions secret APPLE_CERTIFICATE
- Password stored in APPLE_CERTIFICATE_PASSWORD secret
- Team ID stored in APPLE_TEAM_ID secret
- CI creates a temporary keychain, imports cert, signs, destroys keychain — standard pattern
- Local dev uses self-signed cert from just setup-signing — no Apple account needed for contributors
