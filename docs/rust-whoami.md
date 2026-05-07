+++
id = "0cedf892-a6b8-444d-9041-fb3d0c531d28"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Implement whoami tool in Rust

## Overview

Multi-provider auth status check: git, GitHub (gh), GitLab (glab), AWS, Kubernetes, OCI registries. Shell out to CLI tools and parse output. Straightforward port.

## Decisions

### Decision: Implemented as core tool with 7 providers

**Status:** decided
**Rationale:** Direct port of TS auth.ts. All 7 providers (git, github, gitlab, aws, k8s, oci, vault) with diagnose_error pattern matching. Runs on spawn_blocking to avoid blocking tokio. No additional crate needed — lives in tools/whoami.rs.

## Open Questions

*No open questions.*
