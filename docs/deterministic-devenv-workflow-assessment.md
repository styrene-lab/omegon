---
id: deterministic-devenv-workflow-assessment
title: "Assess devenv.sh for deterministic Omegon developer workflow"
status: deferred
tags: [developer-workflow, determinism, release-hardening, tooling]
open_questions:
  - "[assumption] Omegon should prefer a Nix/devenv-managed shell for deterministic development while keeping plain Cargo/just commands usable for contributors who do not use Nix."
  - "Which tools must be pinned for meaningful parity: Rust toolchain, cargo-zigbuild, Zig, cargo-cyclonedx, cargo-license, Node/npm/tsx for extensions, Python, GitHub CLI, Docker/Podman, cosign, just, or all release-path tools?"
  - "Should devenv.sh own only interactive developer shells, or should CI/release workflows also consume the same lockfile/config to prevent drift?"
  - "How should macOS signing/notarization and Linux cross-build dependencies be represented without leaking secrets or requiring heavyweight release-only tools for every contributor shell?"
dependencies: []
related:
  - nex-substrate-tool-boundary
  - nex-deterministic-substrate-boundary

---

# Assess devenv.sh for deterministic Omegon developer workflow

## Overview

Evaluate whether adding devenv.sh should become part of Omegon's deterministic development and release workflow. The goal is to reduce host drift across Rust toolchain, cargo helpers, release utilities, Python scripts, Node-backed extension tooling, and CI-local parity without over-constraining contributor onboarding.

## Research

### Current workflow and drift evidence

Project commands are wrapped in `just` (`just test-rust`, `just lint`, `just build`, `just link`, `just preflight`), while release CI installs tools dynamically (`cargo-cyclonedx`, `cargo-license`, `cargo-zigbuild`, Zig, GitHub Actions Node-backed actions). Recent v0.25.5 release repair showed drift risk around license audit tooling and release-gap detection. A deterministic dev shell could make local preflight closer to CI by pinning versions of Rust helper tools and scripting runtimes.

## Decisions

### Use devenv.sh as an optional deterministic shell, not a mandatory contributor gate

**Status:** candidate

**Rationale:** This preserves the existing Cargo/just onboarding path while giving release maintainers and heavy contributors a pinned environment. Mandatory Nix/devenv adoption would improve reproducibility but raises contributor friction and can complicate macOS signing and container workflows.

### Scope initial devenv to local preflight parity before release-build parity

**Status:** candidate

**Rationale:** The first useful slice is pinning tools needed for `just preflight`, license audit, release-gap checks, Rust checks, Python scripts, and extension/typecheck tooling. Full release parity for notarization, OCI publishing, and cross-architecture signing can be assessed later because those paths require secrets and platform-specific capabilities.

### Keep justfile as the command interface over the devenv implementation

**Status:** candidate

**Rationale:** `just` is already the project command API. devenv should provide the deterministic tool substrate, while `just` remains the human and CI command surface. This avoids making developers learn two task systems and keeps future workflow changes localized.

## Open Questions

- [assumption] Omegon should prefer a Nix/devenv-managed shell for deterministic development while keeping plain Cargo/just commands usable for contributors who do not use Nix.
- Which tools must be pinned for meaningful parity: Rust toolchain, cargo-zigbuild, Zig, cargo-cyclonedx, cargo-license, Node/npm/tsx for extensions, Python, GitHub CLI, Docker/Podman, cosign, just, or all release-path tools?
- Should devenv.sh own only interactive developer shells, or should CI/release workflows also consume the same lockfile/config to prevent drift?
- How should macOS signing/notarization and Linux cross-build dependencies be represented without leaking secrets or requiring heavyweight release-only tools for every contributor shell?
