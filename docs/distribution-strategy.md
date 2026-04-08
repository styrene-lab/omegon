# Distribution Strategy

## Overview

Map all distribution channels for the omegon binary: install.sh, GitHub Releases, Homebrew tap, and signing/notarization for macOS Gatekeeper.

## Current Channels

| Channel | Status | Signing | Verification |
|---|---|---|---|
| `install.sh` | Working | SHA-256 mandatory, cosign optional | Automatic |
| GitHub Releases | Working | cosign keyless + SBOM + attestations | Manual |
| npm (`omegon`) | Working | npm provenance | Automatic |
| Local build | Working | Developer ID (YubiKey) | `codesign -dvvv` |

## Signing Pipeline

### Developer ID (Local — YubiKey)
- **Certificate**: Developer ID Application: CHRISTOPHER RYAN WILSON (UZBY9DM42N)
- **Slot**: 9c on YubiKey 5.4.3
- **Tool**: rcodesign with --smartcard-slot 9c
- **Flags**: --code-signature-flags runtime (hardened runtime for notarization)

### Notarization (Apple)
- **Requirement**: App Store Connect API key (one-time setup)
- **Flow**: sign → zip → notary-submit → wait → staple
- **Tool**: rcodesign notary-submit (uses API key, not Apple ID)
- **Result**: Binary passes Gatekeeper on any Mac without warnings

### Setup Steps
1. Generate API key at https://appstoreconnect.apple.com/access/integrations/api
2. Download the .p8 file
3. Store credentials: `xcrun notarytool store-credentials "omegon" --apple-id EMAIL --team-id UZBY9DM42N --key-id KEY_ID --key PATH_TO_P8`
4. Or for rcodesign: set APPLE_API_KEY, APPLE_API_ISSUER env vars

### CI Notarization
The release.yml workflow can notarize macOS binaries if we add the API key as a GitHub secret. The flow:
1. Build binary for aarch64-apple-darwin and x86_64-apple-darwin
2. codesign with Developer ID (requires cert in CI — export from YubiKey or use rcodesign's CI mode)
3. Submit to notary service
4. Staple the ticket
5. Archive as .tar.gz

**Problem**: Developer ID signing in CI requires the private key accessible. Options:
- Export cert+key from YubiKey to a PKCS12 file, store as GitHub secret
- Use rcodesign's --p12-file flag in CI
- Or: only notarize locally (sign+notarize in `just sign`, CI does cosign only)

### Recommended: Hybrid Signing
- **CI** (automated): cosign keyless + SBOM + attestations (supply chain)
- **Local** (manual): Developer ID + notarize + staple (macOS Gatekeeper)
- **Homebrew**: points at GitHub Releases (cosign-signed); macOS users who install via brew won't hit Gatekeeper because brew handles quarantine

## Homebrew Tap

Formula at `homebrew/Formula/omegon.rb`. Auto-updated by `.github/workflows/homebrew.yml` on stable releases.

Install: `brew tap styrene-lab/tap && brew install omegon`

## Linux ABI compatibility

Homebrew on Linux is a package delivery channel, not a guarantee that Omegon's release binary will run against the host glibc.

If a release artifact is built on too new a Linux baseline, users may install successfully and then fail at runtime with missing symbol versions such as `GLIBC_2.38` or `GLIBC_2.39`.

### Current packaging reality

- Homebrew does **not** patch the host's system glibc to satisfy Omegon
- a Linux release built against a newer glibc will fail on older distros even when install succeeds
- install docs must describe this honestly until release artifacts target a lower common baseline or a musl/static path exists

### Release requirement

The release process should eventually guarantee at least one Linux distribution path that avoids this footgun:

- build on an older glibc baseline, or
- ship a musl/static Linux artifact where practical, or
- publish explicitly versioned distro/ABI-targeted Linux artifacts

Until then, Linux Homebrew documentation must be treated as conditional on host glibc compatibility.

## Open Questions

- Should we export the Developer ID private key for CI notarization, or keep it YubiKey-only (local signing)?
- Do we need a universal (fat) binary for macOS, or are separate arm64/x86_64 fine?
