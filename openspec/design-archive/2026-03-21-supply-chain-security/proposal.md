+++
id = "898f35d9-9262-41b6-b996-35e4956baae8"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Supply chain security — code signing, SBOM generation, and release provenance for Rust binary

## Intent

The Rust binary ships via GitHub Releases with SHA256 checksums but no code signing, no SBOM, and no provenance attestation. The npm package has sigstore provenance via `npm publish --provenance`, but the Rust binary — the actual thing operators run — has none of these. This node covers three layers: SBOM generation (what's in the binary), code signing (who built it), and provenance attestation (how it was built).

See [design doc](../../../docs/supply-chain-security.md).
