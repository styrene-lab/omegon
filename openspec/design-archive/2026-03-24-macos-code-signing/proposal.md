+++
id = "3f3cbc8c-e363-49bc-b3aa-c69b9ca39200"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# macOS code signing — stable identity for keychain ACL persistence and Gatekeeper

## Intent

macOS Keychain grants 'Always Allow' based on binary CDHash. Unsigned or ad-hoc signed binaries get a new CDHash every build, so operators must re-authorize keychain access on every RC. This requires three tiers of signing: (1) local dev self-signed cert for contributors, (2) Apple Developer ID for public releases, (3) future notarization for Gatekeeper. The signing identity must be stable across builds but distinct from Styrene Identity (which is operator-level, not publisher-level).

See [design doc](../../../docs/macos-code-signing.md).
