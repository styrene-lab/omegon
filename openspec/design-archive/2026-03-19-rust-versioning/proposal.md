+++
id = "b7c6dec2-d4c2-448a-b048-2067952eee0c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust versioning system — semver, changelog, --version, release workflow

## Intent

Set up a proper versioning system for the Rust-primary omegon-core repo. Current state: workspace.version = 0.12.0, one git tag (v0.12.0), no --version CLI flag, no CHANGELOG, no automated version bumping. Release CI already builds 4 targets and publishes to GitHub Releases on tag push.

See [design doc](../../../docs/rust-versioning.md).
