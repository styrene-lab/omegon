+++
id = "9d477099-d8ee-4a11-b660-e17b4229cd17"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Full SBOM signing and verification pipeline

## Overview

End-to-end SBOM signing: CycloneDX generation (already in CI), cosign signature on the SBOM (already in CI), local SBOM generation via just recipe, verification tooling for consumers (cosign verify-blob on SBOM + attestation check). Also: reproducible build investigation, SLSA Level 3 compliance check.

## Open Questions

*No open questions.*
