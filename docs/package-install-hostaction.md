---
id: package-install-hostaction
title: "package.install@1 HostAction"
status: exploring
tags: [host-actions, extensions, sdk, security, package-install]
open_questions:
  - "Should package.install@1 target only Omegon extensions/providers in v1, or also language package ecosystems such as cargo/pip/npm?"
  - "What is the minimum install result contract: installed package id/version only, or also filesystem paths, registry/source digest, and activation instructions?"
  - "Should rollback be explicit unsupported in v1, or should successful installs record enough state for uninstall/rollback follow-up actions?"
dependencies: []
related: []
---

# package.install@1 HostAction

## Overview

Design the package.install@1 HostAction as a host-mediated, approval-gated package acquisition primitive for trusted runtime capabilities such as provider plugins/extensions. It must not be a generic shell installer or terminal wrapper.

## Research

### Ecosystem readiness

SDK extraction is complete enough to stop iterating on extraction itself. Current ecosystem shape: Omegon host depends on crates.io omegon-extension = "0.25"; omegon-extension-rs owns canonical Rust SDK source and sdk-contract.json; Python/TypeScript SDKs consume the same contract artifact; omegon-nex-rs is a Rust-native trusted provider prototype waiting on host package.install@1 execution support.

## Decisions

### package.install@1 is package-domain, not command-domain

**Status:** decided

**Rationale:** The action represents host-mediated acquisition/activation of a named runtime capability. It must not accept arbitrary shell commands or package-manager invocations; those belong to terminal/process domains with different risk and observability.

### v1 supports dry-run planning before mutation

**Status:** decided

**Rationale:** Install actions mutate the operator environment. v1 must support dry_run/plan mode so extensions and approval cards can show exactly what would be installed, from where, and what files/registries would be touched before any mutation.

### Manual approval required by default

**Status:** decided

**Rationale:** Even trusted packages can introduce executable code and persistent capability changes. package.install@1 must route through HostAction approval by default; future auto-install can be considered only for tightly allowlisted sources/packages with clear audit evidence.

### Initial sources limited to registry and local path

**Status:** decided

**Rationale:** extension_registry/package_registry and local_path are bounded enough for initial policy and tests. Generic GitHub repos, curl scripts, and arbitrary system package managers are excluded from v1 because provenance and execution semantics are too broad.

### Install execution starts as plan-only for omegon-nex-rs

**Status:** proposed

**Rationale:** omegon-nex-rs can validate the end-to-end HostAction request, approval payload, and dry-run result without immediately mutating the operator system. Real installation can follow after policy and audit are proven.

## Open Questions

- Should package.install@1 target only Omegon extensions/providers in v1, or also language package ecosystems such as cargo/pip/npm?
- What is the minimum install result contract: installed package id/version only, or also filesystem paths, registry/source digest, and activation instructions?
- Should rollback be explicit unsupported in v1, or should successful installs record enough state for uninstall/rollback follow-up actions?
