+++
id = "78c0d399-3b41-4d28-a6cf-44f03aa7b707"
kind = "document"
title = "Supply chain security — code signing, SBOM generation, and release provenance for Rust binary"
status = "implemented"
tags = ["security", "distribution", "signing", "sbom", "sigstore", "ci", "release", "supply-chain"]
aliases = ["supply-chain-security"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
priority = "1"
+++

# Supply chain security — code signing, SBOM generation, and release provenance for Rust binary

## Overview

The Rust binary ships via GitHub Releases. This node covers three layers of release assurance: SBOM generation (what is in the binary), code signing (who built it), and provenance attestation (how it was built).

## Research

### Current state and three layers needed



### Release workflow design — the missing piece

The release workflow must build and publish platform-specific archives and checksums consumed by install.sh.

### Proposed: `.github/workflows/release.yml`

Triggered when a `v*` release tag is pushed.

**Matrix build:**
```yaml
strategy:
  matrix:
    include:
      - target: x86_64-unknown-linux-gnu
        os: ubuntu-latest
      - target: aarch64-unknown-linux-gnu
        os: ubuntu-latest  # cross-compile
      - target: x86_64-apple-darwin
        os: macos-13
      - target: aarch64-apple-darwin
        os: macos-14  # Apple Silicon runner
```

**Steps per matrix entry:**
1. Checkout + install Rust toolchain + target
2. `cargo build --release --target $TARGET -p omegon`
3. Strip binary (`strip` or `llvm-strip`)
4. Create archive: `omegon-$TARGET.tar.gz` (or `.zip` for Windows)
5. Generate SHA256 checksum

**Post-matrix steps (runs once after all builds):**
6. Collect all archives + checksums into `checksums.sha256`
7. Generate SBOM: `cargo cyclonedx --manifest-path Cargo.toml --format json`
8. Sign all artifacts with cosign: `cosign sign-blob --yes <each archive>`
9. Attest build provenance: `actions/attest-build-provenance`
10. Create GitHub Release with all artifacts attached:
    - `omegon-x86_64-unknown-linux-gnu.tar.gz` + `.sig` + `.bundle`
    - `omegon-aarch64-unknown-linux-gnu.tar.gz` + `.sig` + `.bundle`
    - `omegon-x86_64-apple-darwin.tar.gz` + `.sig` + `.bundle`
    - `omegon-aarch64-apple-darwin.tar.gz` + `.sig` + `.bundle`
    - `checksums.sha256`
    - `omegon-sbom.cdx.json`
    - SLSA provenance attestation

### install.sh updates

The install script already handles checksums. Add:
1. **Optional cosign verification:** if `cosign` is in PATH, verify the signature. If not, warn but proceed (don't force cosign as a dependency).
2. **SBOM download:** `--sbom` flag downloads and displays the SBOM alongside the binary.

### Container image signing

The site container (omegon.styrene.dev) is already deployed via ArgoCD. If we sign the OCI image with cosign, ArgoCD can enforce signature verification — only signed images deploy.

```yaml
- name: Sign container image
  run: cosign sign --yes ghcr.io/styrene-lab/omegon-site:$TAG
```

## Decisions

### Decision: cargo-cyclonedx for SBOM, cosign keyless for signing, GitHub attest for provenance

**Status:** decided
**Rationale:** Three stable, well-supported tools that compose: cargo-cyclonedx generates CycloneDX SBOM from Cargo.lock (stable Rust, dominant standard). Cosign keyless uses GitHub Actions OIDC for signing (zero key management, transparent via Rekor). actions/attest-build-provenance for provenance (GitHub-native, simpler than full SLSA). All three run in CI with no secrets to manage beyond GITHUB_TOKEN (which is automatic).

### Decision: Release workflow triggered by v* tag, matrix build for 4 targets

**Status:** decided
**Rationale:** release.yml triggers on the release tag. Matrix: x86_64-linux (ubuntu), aarch64-linux (cross-compile), x86_64-macos, and aarch64-macos. Each target produces a stripped binary in a tar.gz. Post-matrix: checksums, SBOM, signing, provenance, and GitHub Release creation.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `.github/workflows/release.yml` (new) — Rust binary release pipeline: matrix build (4 targets), strip, archive, SHA256 checksums, CycloneDX SBOM, cosign keyless signing (blob + certificate), actions/attest-build-provenance, GitHub Release creation with all artifacts.
- `core/install.sh` (modified) — Optional cosign signature verification: if cosign is in PATH, verify .sig + .pem from release. If cosign missing, informational message (not blocking).
- `Justfile` (modified) — Added sbom recipe (cargo cyclonedx) and verify-sig recipe (cosign verify-blob).

### Constraints

- Cosign verification in install.sh is optional — doesn't block install if cosign not present
- SBOM is CycloneDX JSON format attached to every GitHub Release
- Signing uses OIDC keyless (GitHub Actions identity) — zero secrets to manage
- Release workflow only triggers on v* tags

## What exists today

**Rust binary:** install.sh downloads from GitHub Releases, verifies SHA256 from `checksums.sha256` file. But:
- No release workflow exists — there's no GitHub Actions job that builds the Rust binary and creates a release. The `checksums.sha256` and archives are presumably created manually or by an untracked process.
- Checksums prove integrity (nobody tampered with the download) but not provenance (who built it, from what source).
- No SBOM — operators can't audit what dependencies are in the binary.

## Three layers needed

### Layer 1: SBOM (what's in the binary)

**Tool:** `cargo-cyclonedx` generates CycloneDX SBOM from Cargo.lock.
- Lists all direct + transitive Rust crate dependencies with versions
- CycloneDX 1.5 format (JSON or XML) — the industry standard
- Consumed by: vulnerability scanners (Trivy, Grype), compliance tools, operator security teams
- **Generated at:** CI build time, attached to the GitHub Release as `omegon-sbom.cdx.json`

**Alternative:** `cargo sbom` (nightly-only Cargo feature, tracking issue #16565) — generates SBOM precursor files alongside compiled artifacts. Not yet stable.

**Recommendation:** `cargo-cyclonedx` — it's stable, CycloneDX is the dominant standard, and it works with stable Rust. Run in CI, attach output to release.

### Layer 2: Code signing (who built it)

**Two approaches:**

**A. Sigstore cosign (keyless):**
- Uses OIDC identity (GitHub Actions' OIDC token) as the signing identity
- No key management — the signing happens via Sigstore's transparency log (Rekor)
- `cosign sign-blob --yes omegon-x86_64-unknown-linux-gnu.tar.gz` → creates `.sig` + `.bundle`
- Verification: `cosign verify-blob --certificate-identity-regexp "github.com/styrene-lab/omegon" --certificate-oidc-issuer "https://token.actions.githubusercontent.com"`
- **Pros:** Zero key management, transparent, auditable via Rekor, free
- **Cons:** Requires cosign CLI for verification (not just sha256sum)

**B. GPG signing:**
- Traditional: generate a GPG key, store it in GitHub Secrets, sign release artifacts
- `gpg --detach-sign --armor omegon-x86_64.tar.gz` → creates `.asc` file
- Verification: `gpg --verify omegon-x86_64.tar.gz.asc`
- **Pros:** Universal (gpg is everywhere), simple verification
- **Cons:** Key management burden, key rotation, key compromise = total loss

**Recommendation:** Sigstore cosign keyless — it's the modern approach, eliminates key management entirely, and GitHub Actions has native OIDC support. GPG as a secondary output for operators who prefer it.

### Layer 3: Provenance attestation (how it was built)

**Tool:** SLSA (Supply chain Levels for Software Artifacts) provenance via `slsa-framework/slsa-github-generator`.
- Generates a signed SLSA provenance attestation that records:
  - Source repo + commit hash
  - Build command + builder identity
  - Input hashes (Cargo.lock, source files)
  - Output hashes (compiled binary)
- Attestation is signed by the GitHub Actions OIDC identity
- Provides SLSA Level 3 guarantees (the build was not tampered with)
- Consumers verify with `slsa-verifier verify-artifact`

**Alternative:** `actions/attest-build-provenance` — GitHub's native provenance action. Simpler than full SLSA but less standardized.

**Recommendation:** Start with `actions/attest-build-provenance` (simpler, GitHub-native), upgrade to full SLSA generator later if enterprise customers need Level 3.
