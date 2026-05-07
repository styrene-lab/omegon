+++
id = "9520b187-20f9-46d3-a92c-dab79e301dc7"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Supply chain security — code signing, SBOM generation, and release provenance for Rust binary — Design Spec (extracted)

> Auto-extracted from docs/supply-chain-security.md at decide-time.

## Decisions

### cargo-cyclonedx for SBOM, cosign keyless for signing, GitHub attest for provenance (decided)

Three stable, well-supported tools that compose: cargo-cyclonedx generates CycloneDX SBOM from Cargo.lock (stable Rust, dominant standard). Cosign keyless uses GitHub Actions OIDC for signing (zero key management, transparent via Rekor). actions/attest-build-provenance for provenance (GitHub-native, simpler than full SLSA). All three run in CI with no secrets to manage beyond GITHUB_TOKEN (which is automatic).

### Release workflow triggered by v* tag, matrix build for 4 targets (decided)

The publish.yml already creates git tags. release.yml triggers on the same tag. Matrix: x86_64-linux (ubuntu), aarch64-linux (cross-compile), x86_64-macos (macos-13), aarch64-macos (macos-14 Apple Silicon). Each target produces a stripped binary in a tar.gz. Post-matrix: checksums, SBOM, signing, provenance, GitHub Release creation.

## Research Summary

### Current state and three layers needed



### Release workflow design — the missing piece

The biggest gap isn't signing or SBOM — it's that **no GitHub Actions workflow builds and releases the Rust binary at all.** The publish.yml only publishes the npm package. The install.sh expects GitHub Releases to exist with platform-specific archives and checksums. Something has to create those.

### Proposed: `.github/workflows/release.yml`

Triggered on: git tag `v*` pushed (same tag that publish.yml creates).

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
1. Checkout + install Rust toolchain + target…

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
