+++
id = "c2b8826a-e156-4255-9fd9-7da772af67cf"
kind = "design_node"
title = "Nex profiles — deterministic sandbox isolation for Omegon agents"
status = "implementing"
tags = ["security", "isolation", "containers", "identity", "nix"]
aliases = ["nex-profiles"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
parent = "null"
priority = "2"
+++

# Nex Profiles

Deterministic, Nix-derived environment specifications that materialize as OCI
containers for agent sandbox isolation. Optionally bound to Styrene Identity
keypairs for cryptographic trust assertions.

## Problem

Child agents (cleave/delegate) run as bare subprocesses on the host. No
filesystem isolation, no resource limits, no identity-scoped trust. A delegate
that runs `rm -rf /` kills the host. Extensions execute on the host too.

## Architecture

```text
NexManifest (TOML)  ──parse──→  NexProfile (Rust types)
                                     │
                    ┌────────────────┼────────────────┐
                    ↓                ↓                ↓
              NexRegistry      materialize()    bind_identity()
           (lookup by name)   (→ podman run)   (→ RuntimeIdentity)
```

A Nex profile is:
1. **Declarative** — TOML manifest specifying packages, tools, resource limits
2. **Deterministic** — same manifest hash = same OCI image (Nix derivation)
3. **Identity-bound** — signed by Styrene Identity keypair (Phase 4)
4. **Materializable** — resolves to `podman run` with full sandbox constraints

## On-disk format

```toml
[profile]
name = "my-project"
base = "coding-python"

[overlays.ml-deps]
packages = ["python312Packages.torch"]

[resources]
memory_mb = 2048
network = "none"

[capabilities]
mount_cwd = true
filesystem_write = true
allowed_tools = ["bash", "read_file", "write_file"]
```

## Phases

- **Phase 0** (done): Rust types — NexProfile, NexManifest, NexRegistry, container materialization
- **Phase 1**: Containerized child agent spawning via spawn_child_process() integration
- **Phase 2**: Profile registry with HarnessStatus reporting + MCP integration
- **Phase 3**: Nix composition for custom profiles (nix/nex.nix)
- **Phase 4**: Styrene Identity binding — Ed25519 signed manifests, HKDF key derivation

## Feature gate

All code behind `#[cfg(feature = "nex")]`. Default builds unaffected.
`cargo build --features=nex` to enable.

## Key files

- `core/crates/omegon/src/nex/` — module directory
- `core/crates/omegon/src/nex/profile.rs` — core types
- `core/crates/omegon/src/nex/manifest.rs` — TOML parsing
- `core/crates/omegon/src/nex/container.rs` — podman/docker command builder
- `core/crates/omegon/src/nex/spawn.rs` — container-aware child spawning
- `core/crates/omegon/src/nex/registry.rs` — built-in + custom profile lookup
