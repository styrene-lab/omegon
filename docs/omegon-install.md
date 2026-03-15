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

## Research

### Runtime dependency analysis

**What omegon needs at runtime from pi-mono:**
- `packages/{coding-agent,agent,ai,tui,web-ui,mom,pods}/dist/` — ~15MB built JS
- `node_modules/` — ~546MB of npm deps (the bulk)
- Extensions import types/utilities from `@cwilson613/pi-{coding-agent,tui,ai}`

**What omegon adds on top:**
- ~1700 source files: extensions/, themes/, skills/, docs/, bin/pi entrypoint
- `bin/pi` sets `PI_CODING_AGENT_DIR` to omegon root and imports `vendor/pi-mono/.../dist/cli.js`

**The blocker for `npm install -g omegon`:**
- npm packages don't include git submodules — `vendor/pi-mono/` would be empty
- The pi-mono packages ARE published to npm (`@cwilson613/pi-coding-agent` etc.)
- Extensions resolve `@cwilson613/*` imports via the pi-mono workspace `node_modules/`

**Key insight:** The @cwilson613 packages are already on npm. If omegon declares them as regular npm dependencies instead of importing from vendor/, `npm install -g omegon` would resolve everything from the registry. The vendor/ submodule becomes a dev-only concern for contributing patches to pi-mono.

### Distribution options

**Option A: npm install -g omegon (registry-first)**
- Declare `@cwilson613/pi-coding-agent` as a regular dependency in omegon's package.json
- `bin/pi` resolves cli.js from `node_modules/@cwilson613/pi-coding-agent/dist/cli.js` instead of `vendor/`
- Extensions already import from `@cwilson613/*` — they'd resolve from omegon's own node_modules
- vendor/pi-mono stays as a devDependency / optional for contributors
- Pros: standard npm install, auto-updates via npm, zero friction
- Cons: requires publishing omegon to npm; pi-mono must be published first for each release

**Option B: curl installer script**
- `curl -fsSL https://omegon.dev/install | sh`
- Script clones repo, runs npm install + npm link
- Pros: works without publishing to npm, full control
- Cons: still a git clone under the hood, needs git+node prereqs

**Option C: Homebrew tap**
- `brew install cwilson613/tap/omegon`
- Formula downloads tarball, runs build
- Pros: familiar macOS install path
- Cons: macOS only, tap maintenance overhead

**Option D: npm install -g + postinstall fetch**
- Publish omegon to npm without vendor/
- postinstall downloads a pre-built pi-mono tarball from GitHub releases
- Pros: npm install works, no submodule
- Cons: fragile postinstall, enterprise proxy issues

**Recommendation: Option A** — simplest, most standard. The @cwilson613 packages are already published. `bin/pi.mjs` uses `vendor/` only in a source checkout (dev mode) and otherwise falls back to `node_modules/` in the installed product.

## Update contract

Omegon is the single installed product boundary. `vendor/pi-mono` is a contributor/dev source of implementation, not a second installed product.

The authoritative update path therefore must:
- mutate the package/runtime surface (`git pull` + submodule sync + build + dependency refresh + `npm link --force` in dev mode, or `npm install -g omegon@latest` in installed mode)
- verify the active `pi` binary still resolves to Omegon
- stop at a deliberate restart handoff

`/refresh` is intentionally narrower: it only clears transient caches and reloads extensions. It is not equivalent to `/update` after package/runtime mutation.

## Decisions

### Decision: Publish as `omegon` (unscoped) to npm

**Status:** decided
**Rationale:** User chose unscoped `omegon`. Simpler install command: `npm i -g omegon`.

### Decision: vendor/ preference with node_modules/ fallback in bin/pi

**Status:** decided
**Rationale:** Dev mode uses vendor/pi-mono (git submodule, latest patches). Installed mode falls back to node_modules/@cwilson613/pi-coding-agent (npm registry). Same bin/pi for both paths.

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
**Rationale:** scripts/preinstall.sh auto-removes @cwilson613/pi-coding-agent and @mariozechner/pi-coding-agent during global install to prevent EEXIST on the `pi` bin link. Clear messaging about what it does and how to revert. Also registers `omegon` as a conflict-free bin alias.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `bin/pi` (modified) — Add existsSync check — vendor/ first, node_modules/ fallback
- `package.json` (modified) — Add @cwilson613/pi-coding-agent + pi-tui + pi-ai as dependencies; add .files or .npmignore
- `.npmignore` (new) — Exclude vendor/, docs/, tests/, .git, .github, design/
- `.github/workflows/publish.yml` (new) — CI: auto-publish omegon to npm on push to main

### Constraints

- omegon name must be available on npm
- pi-mono fork packages must be published to npm before omegon can depend on them
- extensions use dynamic imports — all .ts extension files must ship in the package
