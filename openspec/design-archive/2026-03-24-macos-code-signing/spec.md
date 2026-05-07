+++
id = "c5a7796b-e158-4e2e-b3d1-0637e6ee3301"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# macOS code signing — stable identity for keychain ACL persistence and Gatekeeper — Design Spec (extracted)

> Auto-extracted from docs/macos-code-signing.md at decide-time.

## Decisions

### Three signing tiers, no tie-ins to Styrene Identity needed (decided)

Apple Developer ID (publisher trust), Sigstore (build provenance), and Styrene Identity (operator trust) are orthogonal. They serve different trust domains and don't share key material. No up-front integration work is needed between Apple signing and Styrene Identity — they can be implemented independently. The only coordination point is macOS Keychain namespace: signing cert → System keychain, operator secrets → login keychain under service name 'omegon'. No collision.

### Developer ID Application certificate for CLI binary distribution (decided)

Developer ID Application is the correct cert type for software distributed outside the Mac App Store. Developer ID Installer is for .pkg installers. We distribute a bare CLI binary via GitHub Releases and install.sh — that's Application, not Installer. If we add a .pkg installer later, we'd generate a second cert.

### Apple cert in GitHub Actions secrets now, Styrene credential wallet later (decided)

CI environments don't have a Styrene Identity — they're headless. The standard pattern (base64-encoded .p12 in APPLE_CERTIFICATE secret, password in APPLE_CERTIFICATE_PASSWORD, Team ID in APPLE_TEAM_ID) works today with no dependencies. When the Styrene Identity credential wallet ships, the cert's private key can be imported retroactively. No ordering dependency — generate and use the cert now, integrate with identity bundle later.

## Research Summary

### Three identity layers and how they interact

There are three distinct identity concepts at play:

**1. Apple Developer ID (publisher identity)**
- WHO: Styrene Lab as the software publisher
- WHAT: Apple Developer certificate for code signing + notarization
- WHERE: CI/CD (GitHub Actions) for release builds, local dev with cert export
- WHY: macOS Gatekeeper requires this to run without the "unidentified developer" warning. Keychain ACL persists across builds signed with the same Developer ID.
- COST: $99/year Apple Developer Program
- SCO…

### Implementation plan — three tiers

**Tier 1: Local dev signing (contributors) — implement now**
- `just setup-signing` creates a self-signed "Omegon Local Dev" certificate
- `just rc` signs the binary with this cert if available, ad-hoc otherwise
- Stable CDHash across builds = macOS Keychain "Always Allow" persists
- No Apple account needed. One-time setup per machine.
- Documented in CONTRIBUTING.md

**Tier 2: Apple Developer ID (public releases) — implement before 0.16.0**
- Apple Developer account already registered
- Create …

### Apple Developer cert as a Styrene Identity credential

The Apple Developer certificate is issued by Apple's CA. You generate a CSR, Apple signs it, you get back a cert + private key (.p12). The cert chains: your cert → Apple WWDR Intermediate → Apple Root CA. macOS trusts it because Apple's root is in the system trust store.

This is fundamentally different from the Styrene Identity's self-sovereign Ed25519 keypair — but it doesn't conflict. They're complementary:

**Styrene Identity as credential wallet:**
The operator's Styrene Identity is the roo…
