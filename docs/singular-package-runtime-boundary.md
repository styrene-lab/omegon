---
id: singular-package-runtime-boundary
title: Singular package runtime boundary and ownership
status: resolved
parent: singular-package-update-lifecycle
open_questions: []
---

# Singular package runtime boundary and ownership

## Overview

> Parent: [Singular package integration and full-lifecycle update parity](singular-package-update-lifecycle.md)
> Spawned from: "What does 'singular package' mean operationally for Omegon installs: one globally installed omegon package that fully owns/update-manages the forked pi runtime, or a thinner wrapper around separately evolving pi artifacts?"

*To be explored.*

## Research

### Runtime entrypoint already encodes singular-package ownership

`bin/pi.mjs` sets `PI_CODING_AGENT_DIR` to the Omegon root and then resolves the pi CLI from exactly one of two internal sources: vendored `vendor/pi-mono/.../dist/cli.js` in dev mode or `node_modules/@cwilson613/pi-coding-agent/dist/cli.js` in installed mode. In both cases the operator invokes the single Omegon-owned `pi`/`omegon` binary entrypoint, so runtime ownership is already centralized at the Omegon package boundary rather than delegated to a separately managed external pi installation.

### Installed-package story is singular in intent but inconsistent in repo source form

The install design in `docs/omegon-install.md` says installed mode should behave as one globally installed `omegon` package that owns the forked pi runtime and falls back to registry `node_modules` when `vendor/` is absent. However the source repo still declares `file:` dependencies into `vendor/pi-mono` for local development, and README language remains partially stale (`/update-pi`, 'packages pi as a dependency') compared with the newer unified `/update` flow. This means the singular-package boundary exists conceptually and at runtime entrypoint resolution, but documentation and source-layout cues still mix dev-workspace mechanics with installed-product semantics.

### Publish/install path excludes vendor and relies on packaged Omegon surface

The repo has a root `.npmignore` excluding `vendor/`, `tests/`, `openspec/`, sessions, and other dev-only paths. A dry-run `npm pack` shows the published tarball contains Omegon's top-level `bin/pi.mjs`, extensions, skills, and prompts rather than the vendored submodule. That supports the intended ownership model: the installed artifact is Omegon itself, not an externally installed standalone pi package. Preinstall logic also removes conflicting standalone pi packages so the `pi` binary belongs to Omegon after global install.

### Integration seams that still need cleanup

Two seams remain visible: (1) README/install/update wording still references older split mental models such as `/update-pi`, and (2) source-level `file:` dependencies plus vendor-first docs can obscure that vendor is a contributor/dev mechanism, not the installed-product contract. The package boundary is therefore operationally singular for users, but not yet crisply communicated or consistently enforced across docs/update semantics.

## Decisions

### Decision: Omegon should be treated as the single installed product boundary, with vendor/pi-mono as a dev-only implementation source

**Status:** decided
**Rationale:** Operators install and invoke Omegon, not a separately managed standalone pi package. `bin/pi.mjs` already centralizes runtime ownership at the Omegon root, preinstall removes conflicting standalone pi binaries, and the publish/install model excludes `vendor/` from the distributed artifact. Therefore the product contract should be one Omegon package that owns runtime/update management, while `vendor/pi-mono` remains a contributor-facing source of fork patches in development.

## Open Questions

*No open questions.*
