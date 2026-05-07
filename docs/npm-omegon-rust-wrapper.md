+++
id = "d80b2248-ea8b-4831-8074-b60596c4cdd1"
kind = "document"
title = "Repurpose `omegon` npm package as Rust binary platform wrapper"
status = "decided"
tags = ["npm", "distribution", "rust"]
aliases = ["npm-omegon-rust-wrapper"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Repurpose `omegon` npm package as Rust binary platform wrapper

## Overview

Now that the TS harness lives at `omegon-pi`, the `omegon` npm package name is free to become a thin wrapper that installs the Rust binary via platform-specific optionalDependencies (`@omegon/darwin-arm64`, etc.). This gives users `npm i -g omegon` → native Rust binary on PATH, same pattern as esbuild/claude-code. The existing platform package scaffolds in `omegon-pi/npm/platform-packages/` can be repurposed or moved to the Rust repo's release pipeline.

## Research

### Existing platform package scaffolds

The `omegon-pi` repo already has `npm/platform-packages/` with scaffolds for darwin-arm64, darwin-x64, linux-arm64, linux-x64 under `@styrene-lab/omegon-*`. These currently target `omegon-agent` binary name. The Rust release pipeline at `styrene-lab/omegon-core` produces binaries via GitHub Releases (v0.13.0 latest). The `core/install.sh` script downloads from GH Releases to `/usr/local/bin/omegon`. The wrapper approach would embed those same binaries into npm platform packages instead.

## Decisions

### Decision: Pipeline lives in omegon-core, scope is @styrene-lab, versions track Rust

**Status:** decided
**Rationale:** No cross-pollination between TS and Rust repos. omegon-core owns the binary build + npm platform package publish. Platform packages use @styrene-lab/omegon-* (scope already owned). npm omegon wrapper version tracks Rust version (0.13.x+).

### Decision: Two packages, no extras syntax needed

**Status:** decided
**Rationale:** npm has no pip-style [extras]. Two separate packages achieve the same goal more cleanly: `npm i -g omegon` = Rust binary, `npm i -g omegon-pi` = TS TUI. Independent versioning, clear identity, no ambiguity. Future: omegon-pi may declare omegon as an optionalDependency so the TUI gets the native binary for cleave dispatch.

## Open Questions

*No open questions.*
