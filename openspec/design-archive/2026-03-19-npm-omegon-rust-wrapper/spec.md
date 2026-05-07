+++
id = "5f50940c-d293-4730-9808-6ff7e1f9aa0c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Repurpose `omegon` npm package as Rust binary platform wrapper — Design Spec (extracted)

> Auto-extracted from docs/npm-omegon-rust-wrapper.md at decide-time.

## Decisions

### Pipeline lives in omegon-core, scope is @styrene-lab, versions track Rust (decided)

No cross-pollination between TS and Rust repos. omegon-core owns the binary build + npm platform package publish. Platform packages use @styrene-lab/omegon-* (scope already owned). npm omegon wrapper version tracks Rust version (0.13.x+).

### Two packages, no extras syntax needed (decided)

npm has no pip-style [extras]. Two separate packages achieve the same goal more cleanly: `npm i -g omegon` = Rust binary, `npm i -g omegon-pi` = TS TUI. Independent versioning, clear identity, no ambiguity. Future: omegon-pi may declare omegon as an optionalDependency so the TUI gets the native binary for cleave dispatch.

## Research Summary

### Existing platform package scaffolds

The `omegon-pi` repo already has `npm/platform-packages/` with scaffolds for darwin-arm64, darwin-x64, linux-arm64, linux-x64 under `@styrene-lab/omegon-*`. These currently target `omegon-agent` binary name. The Rust release pipeline at `styrene-lab/omegon-core` produces binaries via GitHub Releases (v0.13.0 latest). The `core/install.sh` script downloads from GH Releases to `/usr/local/bin/omegon`. The wrapper approach would embed those same binaries into npm platform packages instead.
