---
id: omegon-install
title: "Omegon Installation & Distribution"
status: implemented
tags: [distribution, dx, packaging]
open_questions: []
---

# Omegon Installation & Distribution

## Overview

Engineers should be able to install omegon with a single command — no git clone, no submodule init, no manual npm link. The challenge: omegon depends on a vendored pi-mono fork (git submodule) and loads extensions/themes/skills from the repo root. Need a distribution strategy that bundles everything into an installable artifact.

## Linux runtime requirements

**Important:** Homebrew on Linux does **not** solve host glibc ABI compatibility for Omegon release binaries.

If a Linux release artifact was built against a newer glibc than your distro provides, install may succeed but the binary will fail immediately at runtime with errors like:

```text
omegon: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found
omegon: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.39' not found
```

That means the host system glibc is older than the binary expects.

### Current expectation

Before relying on a Linux Homebrew install, verify that your host glibc is new enough for the shipped release artifact:

```bash
ldd --version
```

If your system glibc is older than the required version for the current release artifact, `brew install` alone is not sufficient.

### What to do if this happens

Use one of these paths:

- run Omegon on a newer Linux distribution with a compatible glibc
- use a container/VM image that provides the required glibc baseline
- use another distribution channel once an older-glibc or musl/static Linux artifact is published

### Documentation contract

Linux install surfaces must state runtime ABI requirements explicitly. `brew install` should never imply that Homebrew will supply a compatible glibc for Omegon binaries on Linux.

## Research

### Runtime dependency analysis

**What omegon needs at runtime from pi-mono:**
- `packages/{coding-agent,agent,ai,tui,web-ui,mom,pods}/dist/` — ~15MB built JS
- `node_modules/` — ~546MB of npm deps (the bulk)
- Extensions import types/utilities from `@styrene-lab/pi-{coding-agent,tui,ai}`

**What omegon adds on top:**
- ~1700 source files: extensions/, themes/, skills/, docs/, canonical `bin/omegon.mjs` entrypoint, and a legacy `bin/pi.mjs` compatibility shim
- `bin/omegon.mjs` sets `PI_CODING_AGENT_DIR` to omegon root and imports `vendor/pi-mono/.../dist/cli.js`

**The blocker for `npm install -g omegon`:**
- npm packages don't include git submodules — `vendor/pi-mono/` would be empty
- The pi-mono fork packages are migrating from `@cwilson613/*` to the styrene-lab-owned npm scope (`@styrene-lab/pi-coding-agent` etc.)
- Extensions resolve the public styrene-lab package imports through either the vendored workspace in dev mode or Omegon's installed `node_modules/` tree
- Older `@cwilson613/*` package names remain transition-only compatibility debt and must not be treated as the long-term release boundary

**Key insight:** Once the styrene-lab-scoped fork packages are published, omegon can declare them as regular npm dependencies instead of importing from vendor/. During the migration window, docs and install/update surfaces should treat `@styrene-lab/*` as authoritative and `@cwilson613/*` as legacy compatibility only. The vendor/ submodule becomes a dev-only concern for contributing patches to pi-mono.

### Distribution options

**Option A: npm install -g omegon (registry-first)**
- Declare `@styrene-lab/pi-coding-agent` as a regular dependency in omegon's package.json
- `bin/omegon.mjs` resolves cli.js from `node_modules/@styrene-lab/pi-coding-agent/dist/cli.js` instead of `vendor/`
- Extensions import from the styrene-lab-scoped public packages — they'd resolve from omegon's own node_modules in installed mode
- vendor/pi-mono stays as a devDependency / optional for contributors
- Pros: standard npm install, auto-updates via npm, zero friction
- Cons: requires publishing omegon to npm; pi-mono must be published first for each release

**Option B: curl installer script**
- `curl -fsSL https://omegon.styrene.io/install.sh | sh`
- Installs the packaged Omegon binary and standard entrypoints
- Pros: fastest path, no git clone, no Node/npm runtime required
- Cons: shell installer policy may be disallowed in some environments

**Option C: Homebrew tap**
- `brew tap styrene-lab/tap && brew install omegon`
- RC lane: `brew install styrene-lab/tap/omegon-rc`
- Pros: familiar macOS install path, separate stable vs RC lanes
- Cons: Homebrew on Linux does not solve host glibc/runtime ABI mismatches

**Option D: npm install -g + postinstall fetch**
- Publish omegon to npm without vendor/
- postinstall downloads a pre-built pi-mono tarball from GitHub releases
- Pros: npm install works, no submodule
- Cons: fragile postinstall, enterprise proxy issues

**Recommendation: Option A** — simplest, most standard. Omegon should depend on the styrene-lab-scoped fork packages for published installs. `bin/omegon.mjs` uses `vendor/` only in a source checkout (dev mode) and otherwise falls back to `node_modules/` in the installed product. The legacy `pi` alias, if present, immediately re-enters that same Omegon-owned entrypoint.

## Update contract

Omegon is the single installed product boundary. `vendor/pi-mono` is a contributor/dev source of implementation, not a second installed product.

The authoritative update path therefore must:
- mutate the installed runtime surface (`/update install`, `brew upgrade omegon`, `brew upgrade styrene-lab/tap/omegon-rc`, or reinstall via `install.sh` depending on channel)
- verify the active `omegon` / `om` executable still resolves to Omegon
- stop at a deliberate restart handoff that tells the operator to relaunch `om` or `omegon`

`/refresh` is intentionally narrower: it only clears transient caches and reloads extensions. It is not equivalent to `/update` after package/runtime mutation.

## Decisions

### Decision: Publish as `omegon` (unscoped) to npm

**Status:** decided
**Rationale:** User chose unscoped `omegon`. Simpler install command: `npm i -g omegon`.

### Decision: vendor/ preference with node_modules/ fallback in the Omegon entrypoint

**Status:** decided
**Rationale:** Dev mode uses vendor/pi-mono (git submodule, latest patches). Installed mode falls back to node_modules/@styrene-lab/pi-coding-agent (npm registry). `bin/omegon.mjs` is the canonical entrypoint for both paths; any `pi` alias must immediately re-enter that same Omegon-owned boundary.

### Decision: CI auto-publish on push to main

**Status:** decided
**Rationale:** User wants auto-publish. Will use GitHub Actions with npm publish on main push, version bump via conventional commits or timestamp suffix.

### Decision: OIDC Trusted Publishing for CI (no tokens)

**Status:** decided
**Rationale:** npm granular tokens still require OTP even with "bypass 2FA" in some configurations. Trusted Publishing via GitHub Actions OIDC (Node 24, npm ≥11.5.1) eliminates all token management. Requires id-token:write permission and trusted publisher config on npmjs.com pointing to styrene-lab/omegon + publish.yml.

### Decision: Repo under styrene-lab GitHub org

**Status:** decided
**Rationale:** Transferred from cwilson613/omegon to styrene-lab/omegon. GitHub auto-redirects old URLs. npm package stays unscoped `omegon`. version-check.ts REPO_OWNER updated to styrene-lab.

### Decision: Preinstall script removes conflicting pi packages

**Status:** decided
**Rationale:** scripts/preinstall.sh auto-removes `@styrene-lab/pi-coding-agent`, legacy `@cwilson613/pi-coding-agent`, and `@mariozechner/pi-coding-agent` during global install to prevent EEXIST on the legacy `pi` bin link while Omegon takes ownership of the canonical `omegon` entrypoint. Clear messaging about what it does and how to revert.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `bin/omegon.mjs` (modified) — Canonical Omegon entrypoint with vendor/ first, node_modules/ fallback
- `bin/pi.mjs` (modified) — Legacy compatibility shim that re-enters the Omegon entrypoint
- `package.json` (modified) — Canonical `omegon` bin plus optional compatibility `pi` alias; add @styrene-lab/pi-coding-agent + pi-tui + pi-ai as dependencies; add .files or .npmignore
- `.npmignore` (new) — Exclude vendor/, docs/, tests/, .git, .github, design/
- `.github/workflows/publish.yml` (new) — CI: auto-publish omegon to npm on push to main

### Constraints

- omegon name must be available on npm
- pi-mono fork packages must be published to npm before omegon can depend on them
- extensions use dynamic imports — all .ts extension files must ship in the package

## Migration note

Until the styrene-lab-scoped fork packages are fully published and trusted-publisher configuration is confirmed for each package, registry installs may still observe older `@cwilson613/*` artifacts. Treat the personal scope as legacy compatibility only; new dependency references, docs, and release automation should target `@styrene-lab/*`.
