---
id: release-candidate-system
title: Release candidate system — identifiable pre-release builds with deployment verification
status: exploring
parent: core-distribution
tags: [release, distribution, versioning, ci, diagnostics]
open_questions:
  - "How should RC builds be distributed to other machines? Options: GitHub release (pre-release tag), install.sh with channel flag (--rc), scp/direct copy, or cargo install from git ref"
  - "What self-diagnostic output should every build expose? Candidates: --version with git sha, --diagnostics dumping tool registry + feature list + provider status, startup banner with build fingerprint"
  - Should the existing release workflow (release.toml + cargo-release + GitHub Actions) be extended, or should RC be a separate lighter-weight path (just build + tag + upload artifacts)?
jj_change_id: vnmporvyqpnowlowqomxokqxxkswtzzn
---

# Release candidate system — identifiable pre-release builds with deployment verification

## Overview

After the duplicate-tool-names incident, it became clear that deploying fixes to another machine has no verification path. The operator can't tell which build is running, whether it includes a specific fix, or what state the tool registry is in at startup.

This node explores a release candidate system that makes pre-release builds identifiable, deployable, and self-diagnosing.

## Research

### Current release infrastructure

- **Version**: workspace-level in `core/Cargo.toml` (currently 0.14.0), `--version` flag via clap derive
- **Release flow**: `cargo release patch --execute` → bumps version, runs git-cliff for CHANGELOG, tags `v{version}`, pushes tag
- **CI**: `.github/workflows/release.yml` triggers on `v*` tags, cross-compiles for 4 targets (x86_64/aarch64 × linux/macos), creates GitHub Release with attestations
- **Install**: `install.sh` pulls latest release from `styrene-lab/omegon-core`, supports `VERSION=` env var for pinning
- **Gap**: No build metadata (git sha, dirty flag, build date) baked into the binary. `--version` only shows the Cargo.toml version string. Two different builds from the same version are indistinguishable.

### Motivating incident

Duplicate tool names caused Anthropic API 400 on the Rust TUI. Fix was applied locally, binary rebuilt, but deploying to another machine had no way to verify: (1) the other machine was running the new binary, (2) the fix was actually included, (3) what tools were registered at startup. The operator had to trust that the binary was correct with zero observability.

### RC versioning mental model

**The RC lifecycle follows semver pre-release conventions with zero new tooling.**

### Version progression

```
0.14.0          ← current stable release
0.14.1-rc.1     ← first RC for next patch (fix under test)
0.14.1-rc.2     ← second RC if rc.1 had issues
0.14.1          ← stable release (rc.N was good)
```

### How to cut an RC

1. `cd core && cargo release patch --execute` but with `-rc.1` suffix — or manually edit `Cargo.toml` workspace version to `0.14.1-rc.1` and tag `v0.14.1-rc.1`
2. Push tag → existing `release.yml` fires, builds all 4 platform targets
3. GitHub Release created as **pre-release** (not "latest")
4. `--version` output: `omegon 0.14.1-rc.1 (3a4b5c6 2026-03-21)`

### How to deploy an RC to another machine

```sh
VERSION=0.14.1-rc.1 curl -fsSL https://omegon.styrene.dev/install.sh | sh
```
Or just `scp` the binary — `omegon --version` proves which build is running.

### Promoting RC → stable

1. If RC is good: bump version to `0.14.1` (remove `-rc.N`), tag `v0.14.1`
2. CI builds again, GitHub Release created as latest
3. `install.sh` (no VERSION pin) picks it up automatically

### What the build fingerprint tells you

```
omegon 0.14.1-rc.1 (3a4b5c6 2026-03-21)     ← clean tagged RC
omegon 0.14.0 (3a4b5c6-dirty 2026-03-21)     ← local dev build with uncommitted changes
omegon 0.14.0 (3a4b5c6 2026-03-21)           ← clean build from that commit, not tagged
```
The sha is the tie-breaker. Two machines showing the same sha are running the same code. Different sha = different code, regardless of version string.

## Decisions

### Decision: Bake git sha + dirty flag + build timestamp into every binary via build.rs

**Status:** exploring
**Rationale:** This is the lowest-cost, highest-value change. A `build.rs` that sets `GIT_SHA`, `GIT_DIRTY`, `BUILD_DATE` env vars at compile time, consumed by `--version` output. Every build becomes uniquely identifiable regardless of the version string. Produces output like `omegon 0.14.0 (3a4b5c6 2026-03-21)` or `omegon 0.14.0 (3a4b5c6-dirty 2026-03-21)`. Standard Rust pattern — rustc itself does this.

### Decision: Add --diagnostics flag for self-diagnosis

**Status:** exploring
**Rationale:** A `--diagnostics` or `--doctor` flag that dumps: build info (version + sha + date), registered tools with owning feature, registered commands, provider/auth status, plugin discovery results, bridge.js path and existence. Runs without starting the TUI or agent loop. Produces structured output (JSON or human-readable) that can be shared for remote debugging. Low implementation cost — all data already exists in `setup.rs`, just needs a pre-loop dump path.

### Decision: RC builds as pre-release GitHub Releases with semver pre-release tags

**Status:** exploring
**Rationale:** Use `v0.14.1-rc.1` tags that trigger the existing release workflow but create a pre-release GitHub Release (not latest). `install.sh` already supports `VERSION=` pinning, so deploying an RC is `VERSION=0.14.1-rc.1 install.sh`. The existing CI builds all 4 platform targets. No new workflow needed — just tag conventions and a `--pre-release` flag on the GitHub Release step. cargo-release already supports pre-release versions.

### Decision: Semver pre-release tags (0.14.1-rc.1) + build fingerprint (sha + date)

**Status:** decided
**Rationale:** Two orthogonal identity axes: the version string (semver, set in Cargo.toml, controls release channel semantics) and the build fingerprint (git sha + dirty + date, baked by build.rs, identifies the exact code). RC uses semver pre-release: `0.14.1-rc.1`. The fingerprint distinguishes builds within the same version. Together: `omegon 0.14.1-rc.1 (3a4b5c6 2026-03-21)`. Implemented via build.rs + clap version override.

## Open Questions

- How should RC builds be distributed to other machines? Options: GitHub release (pre-release tag), install.sh with channel flag (--rc), scp/direct copy, or cargo install from git ref
- What self-diagnostic output should every build expose? Candidates: --version with git sha, --diagnostics dumping tool registry + feature list + provider status, startup banner with build fingerprint
- Should the existing release workflow (release.toml + cargo-release + GitHub Actions) be extended, or should RC be a separate lighter-weight path (just build + tag + upload artifacts)?
