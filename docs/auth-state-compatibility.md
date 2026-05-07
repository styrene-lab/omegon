+++
id = "a1580c6b-23cc-4590-8b1a-0e99537e443e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auth state compatibility — preserve existing pi/Claude Code logins in Omegon

## Overview

Separate Omegon-owned packaged resources from persistent user auth/settings/session state so installing or updating Omegon reuses existing ~/.pi/agent credentials instead of requiring provider re-login.

## Research

### Current regression cause

Installed Omegon currently sets PI_CODING_AGENT_DIR to omegonRoot in bin/omegon.mjs. In vendored pi-coding-agent, getAgentDir() controls auth.json, settings.json, sessions/, packages/, and global AGENTS.md lookup. That means auth/settings/session persistence move into the installed package directory instead of stable ~/.pi/agent state, so users who already logged into providers in Claude Code/pi do not inherit that auth when launching Omegon.

## Decisions

### Decision: Omegon must keep user state in ~/.pi/agent while loading package resources from the installed Omegon root

**Status:** decided
**Rationale:** Provider auth, settings, sessions, and caches are durable user state and must survive install location changes, updates, and migration from Claude Code/pi. Omegon-specific extensions, skills, prompts, and themes should still load from the installed package root, but that resource discovery must be decoupled from the agent state directory.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `bin/omegon.mjs` (modified) — Stop forcing PI_CODING_AGENT_DIR to omegonRoot for normal installed execution; instead bootstrap Omegon package resources into stable user settings and migrate legacy package-root auth/settings if needed.
- `extensions/bootstrap/index.ts` (modified) — Align binary verification and any lifecycle/update assumptions with the new split between package resources and user state.
- `tests/bin-where.test.ts` (modified) — Cover state-dir vs package-resource behavior and legacy migration expectations.
- `vendor/pi-mono/packages/coding-agent/src/core/resource-loader.ts` (modified) — If necessary, support loading global context/system prompt resources from explicit package roots while keeping agentDir as durable state root.
- `vendor/pi-mono/packages/coding-agent/src/core/package-manager.ts` (modified) — If necessary, support package-root resource registration via settings/packages without mutating durable user state layout.
- `extensions/bootstrap/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/auth-state-compatibility/specs/runtime/auth-state.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/auth-state-compatibility/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `docs/auth-state-compatibility.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Installing or updating Omegon must preserve existing ~/.pi/agent/auth.json login state from Claude Code/pi.
- Installed Omegon must still load its own bundled extensions, skills, prompts, and themes without requiring users to hand-edit settings.
- Migration must safely absorb users who already ran affected versions that stored auth/settings under the package root.
- Installing or updating Omegon preserves existing ~/.pi/agent/auth.json login state from Claude Code/pi.
- Installed Omegon still loads bundled Omegon resources without requiring settings edits.
- Legacy package-root auth/settings/session state migrates forward safely when shared state is absent.

## Acceptance Criteria

### Falsifiability

- This decision is wrong if: `bin/omegon.mjs --where` or equivalent runtime metadata must distinguish the packaged Omegon root from the persistent state directory and show that installed mode no longer treats `omegonRoot` as the agent state root.
- This decision is wrong if: Regression tests must fail if installed Omegon points auth/settings persistence at the package root or if package resources stop loading after switching state back to `~/.pi/agent`.
- This decision is wrong if: The fix must be verifiable from a clean install path and from a legacy-package-state migration path.
