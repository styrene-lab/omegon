+++
id = "17c4dfa4-a6af-4239-9946-5dd93a923ce0"
kind = "document"
title = "Rust versioning system — semver, changelog, --version, release workflow"
status = "implemented"
tags = ["versioning", "release", "ci", "rust", "semver"]
aliases = ["rust-versioning"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
open_questions = []
+++

# Rust versioning system — semver, changelog, --version, release workflow

## Overview

Set up a proper versioning system for the Rust-primary omegon-core repo. Current state: workspace.version = 0.12.0, one git tag (v0.12.0), no --version CLI flag, no CHANGELOG, no automated version bumping. Release CI already builds 4 targets and publishes to GitHub Releases on tag push.

## Research

### Version scheme analysis

**Current state**: 0.12.0 — accumulated through the TypeScript era. The Rust rewrite is architecturally a different product but ships to the same users via the same `install.sh` and GitHub Releases URL.

**Options considered**:

1. **Reset to 0.1.0** — Acknowledges Rust as a new product. Problem: `install.sh` and any version-check logic would interpret 0.1.0 as a downgrade from 0.12.0. Users who `curl | sh` would skip it. The version-check-downgrade-guard design node exists precisely because this class of problem already bit us.

2. **Jump to 1.0.0** — Signals stability. Problem: the project is still actively implementing features (7 nodes in "implementing" status). Premature 1.0 burns the semver social contract — every breaking change in the agent loop, TUI protocol, or plugin interface would force a major bump during a period of high churn.

3. **Continue from 0.12.0, target 1.0.0 as a milestone** — No version confusion, no downgrade traps. 0.13 adds versioning infrastructure, 0.14+ continues feature work, 1.0.0 marks "the agent loop, TUI, plugin interface, and memory system are stable enough that breaking changes are deliberate, not incidental." Pre-1.0 semver convention (MINOR = potentially breaking, PATCH = safe) is already understood by the user base.

**Recommendation**: Option 3. Continue from 0.12.0. Define explicit 1.0 criteria later as a design node when the implementing backlog thins out.

### Release pipeline architecture — ArgoCD on Brutus

**Constraint**: GitHub Actions are not acceptable for the build/release pipeline. ArgoCD workflows on the Brutus cluster handle CI/CD. The cluster can likely compile faster than the local Mac and won't bottleneck the pipeline.

**Current GitHub Actions** (to be replaced):
- `ci.yml`: test + clippy on push/PR to main
- `release.yml`: tag-triggered 4-target cross-compile → GitHub Release
- `site.yml`: deploy omegon.styrene.dev

**Pipeline stages needed**:
1. **Gate** — conventional commit validation, `cargo test --all`, `cargo clippy --all -- -D warnings`
2. **Build** — cross-compile for 4 targets (darwin-arm64, darwin-x64, linux-x64, linux-arm64)
3. **Package** — tar.gz + checksums, container image if applicable
4. **Release** — create GitHub Release, upload artifacts
5. **Publish docs** — generate and deploy documentation site
6. **Update install.sh / site** — ensure omegon.styrene.dev reflects the new version

**Cross-compilation on Brutus**: The cluster likely runs Linux. Cross-compiling for macOS from Linux requires either:
- `cross` (Docker-based, already used in the GitHub Actions matrix)
- A macOS runner (unlikely on k8s)
- Pre-built macOS toolchains via `osxcross`

`cross` is the pragmatic choice — it already works in the existing release.yml and runs in containers, which fits k8s/ArgoCD natively.

**Trigger model**: ArgoCD watches the repo. On tag push matching `v*`, the pipeline fires. Local workflow: `cargo release` bumps Cargo.toml, updates CHANGELOG, commits, tags, pushes. ArgoCD takes it from there.

### Documentation system

**What needs to exist**:
- **User guide / getting started** — installation, first session, key commands, configuration
- **CLI reference** — auto-generated from clap `--help`, all subcommands and flags
- **Tool reference** — each agent tool's description, parameters, examples
- **Architecture docs** — the design tree already captures design decisions; these need a publishable form
- **Reference architecture diagrams** — system overview, data flow, provider topology, lifecycle state machines
- **Changelog** — Keep a Changelog format, auto-generated from conventional commits between tags
- **API docs** — `cargo doc` for crate-level Rust docs (relevant for plugin authors)

**Documentation tooling options**:
- **mdBook** — Rust ecosystem standard, markdown-based, generates static HTML. Used by Rust Book, Tokio, etc. Supports search, themes, custom preprocessors. Fits the existing markdown-heavy workflow.
- **Zola** — Static site generator in Rust. More design flexibility but more overhead.
- **Docusaurus** — JS-based, would reintroduce Node dependency. Not aligned with Rust-primary direction.

**Recommendation**: mdBook for user-facing docs. `cargo doc` for API docs. Both generated in the release pipeline and deployed alongside the install site. Architecture diagrams rendered via D2 (already used in the style skill) or the native SVG diagram backend.

**Changelog generation**: `git-cliff` is the Rust ecosystem standard — parses conventional commits, generates Keep a Changelog format, highly configurable. Integrates with `cargo-release` via pre-release hooks.

### Local release workflow — cargo-release + git-cliff

**cargo-release** handles the local side:
1. Bumps `workspace.package.version` in root Cargo.toml (all crates inherit)
2. Runs pre-release hooks (git-cliff to update CHANGELOG.md)
3. Commits: `chore(release): bump version to X.Y.Z`
4. Tags: `vX.Y.Z`
5. Pushes commit + tag to origin

**Configuration** (`release.toml` or `Cargo.toml [workspace.metadata.release]`):
```toml
[workspace.metadata.release]
shared-version = true
tag-name = "v{{version}}"
tag-message = "v{{version}}"
pre-release-commit-message = "chore(release): bump version to {{version}}"
pre-release-hook = ["git-cliff", "--tag", "v{{version}}", "-o", "CHANGELOG.md"]
pre-release-replacements = []
publish = false  # we don't publish to crates.io
push-remote = "origin"
```

**git-cliff** configuration (`cliff.toml`):
- Groups commits by type (feat → Added, fix → Fixed, etc.)
- Filters out chore/ci/docs unless they're significant
- Links to GitHub compare URLs between tags
- Conventional commit parser with scope support

**Workflow**: `cargo release minor` (or patch/major) does everything. Developer reviews the generated CHANGELOG diff, confirms, and the push triggers ArgoCD.

## Decisions

### Decision: Continue from 0.12.0, target 1.0.0 as a future stability milestone

**Status:** decided
**Rationale:** Resetting to 0.1.0 creates downgrade traps for install.sh and existing users. Jumping to 1.0.0 is premature with 7 nodes still implementing. Continuing from 0.12.0 avoids confusion and gives 1.0.0 real meaning as a stability commitment.

### Decision: cargo-release + git-cliff for local release workflow

**Status:** decided
**Rationale:** cargo-release handles the mechanical version bump, changelog generation (via git-cliff hook), commit, tag, and push in one command. Developer reviews the changelog diff and confirms. The tag push is the handoff point to ArgoCD. No GitHub Actions in the critical path.

### Decision: ArgoCD on Brutus replaces GitHub Actions for build, release, and docs

**Status:** decided
**Rationale:** GitHub Actions are not acceptable. Brutus cluster has ArgoCD workflows already configured. Cross-compilation uses `cross` (Docker-based, already proven in the existing matrix). Pipeline: gate (test+clippy+commitlint) → build (4 targets) → package → GitHub Release → docs deploy → site update.

### Decision: mdBook for user docs, cargo doc for API docs, git-cliff for changelog

**Status:** decided
**Rationale:** mdBook is the Rust ecosystem standard — markdown-based, supports search, themes, preprocessors. Keeps the project Node-free. cargo doc covers crate-level API reference. git-cliff generates Keep a Changelog from conventional commits. Architecture diagrams via D2 or the native SVG backend. All generated in the release pipeline and deployed to omegon.styrene.dev.

### Decision: Enforce conventional commits in CI gate

**Status:** decided
**Rationale:** Conventional commits are the only acceptable commit type. The ArgoCD gate stage validates commit messages against the conventional commit regex. This is a hard gate — non-conforming commits fail the pipeline. git-cliff depends on this discipline for changelog accuracy.

### Decision: Local release flow must be fully non-interactive — zero human confirmation gates

**Status:** decided
**Rationale:** This is an agent harness developed by agents. If a human has to touch a confirmation prompt or review a changelog diff for every push, that's a blocking impediment to every release. cargo-release must run with --no-confirm (or equivalent --execute flag). git-cliff generates the changelog deterministically from conventional commits — no editorial review needed because the commit discipline is enforced upstream. The entire flow from `cargo release patch` to tag arriving at origin must be a single non-interactive command.

### Decision: mdserve for user docs (replaces mdBook decision), cargo doc for API docs

**Status:** decided
**Rationale:** mdserve is already in the toolchain, serves markdown with Mermaid + syntax highlighting + live reload, and is a single Rust binary. Simpler than mdBook for our needs — docs are plain markdown in docs/, preview with `mdserve docs/`, deployed as part of the release container image. cargo doc generates the Rust API reference.

## Open Questions

*No open questions.*
