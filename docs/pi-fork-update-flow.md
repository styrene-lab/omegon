---
id: pi-fork-update-flow
title: "Pi Fork Update & Deploy Flow"
status: implemented
tags: [dx, tooling, pi-mono, workflow]
open_questions: []
---

# Pi Fork Update & Deploy Flow

## Overview

Cross-cutting changes now span three layers: omegon extensions/themes (pi package), pi-mono source (forked core), and the globally-installed pi binary. Today's update flow is manual cp of dist files — fragile, undiscoverable, and won't scale. Need a defined update flow that's low-friction for iterative development and explicit enough to not accidentally break a running pi session.

## Research

### Change taxonomy — three categories with different update costs

**Category A — omegon-only** (extensions, alpharius.json, docs)
- Already handled: `defaults.ts` deploys `alpharius.json` → `~/.pi/agent/themes/` on every session_start
- Extensions load from omegon package dir at startup — no copy needed
- Cost: restart pi

**Category B — pi-mono core only** (tool-execution.ts, diff.ts, bash.ts, theme.ts, etc.)
- Current: manual `cp dist/*.js /opt/homebrew/.../dist/`
- Painful — must know which files changed, easy to miss .map files or related files

**Category C — cross-cutting** (today's work: new ThemeBg vars in theme.ts + new colors in alpharius.json)
- Both Category A and B steps, in dependency order
- Most error-prone: can deploy alpharius.json with new color names before theme.ts knows about them, causing runtime errors

### Option analysis — four approaches for pi-mono dist deployment

**Option 1: dist symlink (recommended)**
Replace `/opt/homebrew/.../pi-coding-agent/dist/` with a symlink to `pi-mono/packages/coding-agent/dist/`.
- Node ESM resolution walks up from the real path of the symlink target → finds `pi-mono/node_modules/@cwilson613/{pi-ai,pi-tui,pi-agent-core}` (workspace links, all present and built)
- `npm run build` in pi-mono is immediately live — zero copy step
- Cost per iteration: `npm run build` (74s currently) + restart pi
- Downside: if pi-mono dist is in a broken state (build halfway), live pi is broken mid-session
- Mitigation: always build before restarting pi (builds are atomic: build writes to dist atomically via tsgo)

**Option 2: `npm link` from pi-mono**
`cd pi-mono/packages/coding-agent && npm link` + `cd homebrew-dir && npm link @cwilson613/pi-coding-agent`
- Elegant in theory; in practice npm link sets up a symlink in the global node_modules — but the pi binary is already a manual homebrew install, not an npm global, so the link target would be wrong.
- Could work if we re-install pi via `npm install -g pi-mono/packages/coding-agent` but that changes the binary path.

**Option 3: `npm pack` + reinstall**
`npm run build && npm pack` in pi-mono, `npm install -g ./pi-coding-agent-0.57.1.tgz` globally.
- Produces a clean install identical to what users get
- Slow (~90s), changes the binary path, breaks the homebrew symlink at `/opt/homebrew/bin/pi`

**Option 4: explicit deploy script**
`scripts/deploy-to-pi.sh` that rsync or selectively copies changed dist files.
- More reliable than manual cp, but still requires knowing what changed
- Can diff before copy to avoid unnecessary writes
- Simpler to understand than symlink magic

**Verdict**: Option 1 (dist symlink) for iterative dev speed. Option 4 as documentation fallback for anyone who finds symlinks confusing. The workspace resolver is already confirmed to work (all @cwilson613 deps present in pi-mono/node_modules).

## Decisions

### Decision: Dist symlink as the primary dev-loop mechanism for pi-mono changes

**Status:** decided
**Rationale:** The homebrew dist dir is replaced once with a symlink to pi-mono/packages/coding-agent/dist/. After that, `npm run build` in pi-mono is the entire update step — zero copy, zero file list to maintain. Node's ESM resolution correctly finds pi-mono/node_modules/@cwilson613/* (workspace links) by walking up from the symlink's real path. A deploy script (scripts/deploy-pi-dev.sh) sets up the symlink and is idempotent, so any contributor can run it once.

### Decision: omegon package.json gets a "build:pi" script pointing to pi-mono

**Status:** decided
**Rationale:** A `npm run build:pi` script in omegon's package.json runs `cd ../pi-mono && npm run build` so contributors don't need to know the relative path. Cross-cutting changes become: edit → `npm run build:pi` (if pi-mono changed) → restart pi. Category A changes still need only a restart.

## Open Questions

*No open questions.*
