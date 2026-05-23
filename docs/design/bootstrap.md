+++
id = "3b307a17-0db7-42ff-a138-3eb06458ae1c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Pi-kit Bootstrap — First-time setup and dependency management

## Disposition — 2026-05-23

**Status: historical-decision / stale implementation scope.** This node describes the earlier Pi-kit TypeScript extension bootstrap model (`pi install`, `deps.ts`, extension session hooks). Those implementation paths are not present in the current Rust-native repository. Current bootstrap/auth/setup behavior should be verified against `core/crates/omegon/src/bootstrap.rs`, `core/crates/omegon/src/auth.rs`, and `core/crates/omegon/src/setup.rs` before using this as guidance.

Use this document only for the durable decision that dependency setup should be centralized and tiered. Do not use its file-scope or Pi extension assumptions as current implementation reference.

## Overview

A generalized bootstrap system that runs after `pi install git:github.com/cwilson613/Omegon`. Each extension declares its external dependencies via a registry. On first session start, bootstrap presents a checklist of what's ready vs missing, and guides the user through interactive setup. Subsequent sessions only warn on newly-missing deps. Also provides `/bootstrap` command for re-running setup.

## Decisions

### Decision: Centralized dep registry with tiered interactive setup

**Status:** decided
**Rationale:** deps.ts is the single source of truth for all external binary dependencies. Each dep declares its check function, install commands, tier (core/recommended/optional), and which extensions use it. Bootstrap extension runs on session_start, detects first-run via marker file, and offers interactive installation grouped by tier. Individual extensions import from deps.ts rather than duplicating checks. Marker file is versioned so adding new core deps re-triggers bootstrap.

## Open Questions

*No open questions.*
