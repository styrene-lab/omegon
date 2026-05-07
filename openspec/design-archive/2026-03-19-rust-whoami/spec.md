+++
id = "fcce176e-705b-40ca-a462-f1f8982bbec0"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Implement whoami tool in Rust — Design Spec (extracted)

> Auto-extracted from docs/rust-whoami.md at decide-time.

## Decisions

### Implemented as core tool with 7 providers (decided)

Direct port of TS auth.ts. All 7 providers (git, github, gitlab, aws, k8s, oci, vault) with diagnose_error pattern matching. Runs on spawn_blocking to avoid blocking tokio. No additional crate needed — lives in tools/whoami.rs.
