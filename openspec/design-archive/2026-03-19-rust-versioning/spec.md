+++
id = "74cec6b4-9160-4c89-ad61-a7c1e5f1e632"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust versioning system — semver, changelog, --version, release workflow — Design Spec (extracted)

> Auto-extracted from docs/rust-versioning.md at decide-time.

## Decisions

### Continue from 0.12.0, target 1.0.0 as a future stability milestone (decided)

Resetting to 0.1.0 creates downgrade traps for install.sh and existing users. Jumping to 1.0.0 is premature with 7 nodes still implementing. Continuing from 0.12.0 avoids confusion and gives 1.0.0 real meaning as a stability commitment.

### cargo-release + git-cliff for local release workflow (decided)

cargo-release handles the mechanical version bump, changelog generation (via git-cliff hook), commit, tag, and push in one command. Developer reviews the changelog diff and confirms. The tag push is the handoff point to ArgoCD. No GitHub Actions in the critical path.

### ArgoCD on Brutus replaces GitHub Actions for build, release, and docs (decided)

GitHub Actions are not acceptable. Brutus cluster has ArgoCD workflows already configured. Cross-compilation uses `cross` (Docker-based, already proven in the existing matrix). Pipeline: gate (test+clippy+commitlint) → build (4 targets) → package → GitHub Release → docs deploy → site update.

### mdBook for user docs, cargo doc for API docs, git-cliff for changelog (decided)

mdBook is the Rust ecosystem standard — markdown-based, supports search, themes, preprocessors. Keeps the project Node-free. cargo doc covers crate-level API reference. git-cliff generates Keep a Changelog from conventional commits. Architecture diagrams via D2 or the native SVG backend. All generated in the release pipeline and deployed to omegon.styrene.dev.

### Enforce conventional commits in CI gate (decided)

Conventional commits are the only acceptable commit type. The ArgoCD gate stage validates commit messages against the conventional commit regex. This is a hard gate — non-conforming commits fail the pipeline. git-cliff depends on this discipline for changelog accuracy.

### Local release flow must be fully non-interactive — zero human confirmation gates (decided)

This is an agent harness developed by agents. If a human has to touch a confirmation prompt or review a changelog diff for every push, that's a blocking impediment to every release. cargo-release must run with --no-confirm (or equivalent --execute flag). git-cliff generates the changelog deterministically from conventional commits — no editorial review needed because the commit discipline is enforced upstream. The entire flow from `cargo release patch` to tag arriving at origin must be a single non-interactive command.

## Research Summary

### Version scheme analysis

**Current state**: 0.12.0 — accumulated through the TypeScript era. The Rust rewrite is architecturally a different product but ships to the same users via the same `install.sh` and GitHub Releases URL.

**Options considered**:

1. **Reset to 0.1.0** — Acknowledges Rust as a new product. Problem: `install.sh` and any version-check logic would interpret 0.1.0 as a downgrade from 0.12.0. Users who `curl | sh` would skip it. The version-check-downgrade-guard design node exists precisely because thi…

### Release pipeline architecture — ArgoCD on Brutus

**Constraint**: GitHub Actions are not acceptable for the build/release pipeline. ArgoCD workflows on the Brutus cluster handle CI/CD. The cluster can likely compile faster than the local Mac and won't bottleneck the pipeline.

**Current GitHub Actions** (to be replaced):
- `ci.yml`: test + clippy on push/PR to main
- `release.yml`: tag-triggered 4-target cross-compile → GitHub Release
- `site.yml`: deploy omegon.styrene.dev

**Pipeline stages needed**:
1. **Gate** — conventional commit validati…

### Documentation system

**What needs to exist**:
- **User guide / getting started** — installation, first session, key commands, configuration
- **CLI reference** — auto-generated from clap `--help`, all subcommands and flags
- **Tool reference** — each agent tool's description, parameters, examples
- **Architecture docs** — the design tree already captures design decisions; these need a publishable form
- **Reference architecture diagrams** — system overview, data flow, provider topology, lifecycle state machines
- **…

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
pre-release-commit-me…
