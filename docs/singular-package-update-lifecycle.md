---
id: singular-package-update-lifecycle
title: Singular package integration and full-lifecycle update parity
status: implemented
parent: pi-fork-update-flow
dependencies: [pi-fork-worktree-reconciliation, singular-package-runtime-boundary, update-lifecycle-parity]
tags: [pi-mono, packaging, update, integration, architecture]
open_questions: []
branches: ["feature/singular-package-update-lifecycle"]
openspec_change: singular-package-update-lifecycle
issue_type: feature
priority: 1
---

# Singular package integration and full-lifecycle update parity

## Overview

Resolve the current pi-mono unstash conflict, audit whether Omegon's forked pi componentry is fully integrated into the singular package, and define/implement an internal update path that matches the full install/update lifecycle as closely as safely possible.

## Research

### Current update path and packaging baseline

Current `/update` behavior in `extensions/bootstrap/index.ts` is not full lifecycle parity with `./scripts/install-pi.sh`. Dev mode performs `git pull`, `git submodule update`, `npm run build` in `vendor/pi-mono`, root `npm install`, cache clear, and `ctx.reload()`. `scripts/install-pi.sh` instead builds pi-mono, runs `npm link --force` at the Omegon root, and verifies the active `pi` binary resolves to Omegon. The current update path does not relink the global binary, does not verify the active binary target, and relies on in-process reload rather than an exit/reexec boundary.

### Singular package intent from existing install design

`docs/omegon-install.md` already defines the desired packaging model: Omegon should behave as a singular package with vendor-first dev resolution and node_modules fallback for installed mode. The design explicitly treats `vendor/pi-mono` as a dev/contributor concern while installed mode should resolve the forked `@cwilson613/*` packages from npm. This implies the integration audit must verify that runtime behavior does not still depend on local submodule state in installed mode.

### Current local fork state requiring reconciliation

The recovered `vendor/pi-mono` workspace is now on its local `main` with unstashed work partially applied. Files restored cleanly include `packages/ai/src/providers/anthropic.ts`, `packages/coding-agent/src/core/agent-session.ts`, `packages/coding-agent/src/core/auth-storage.ts`, and `packages/coding-agent/src/modes/interactive/components/tool-execution.ts`. `packages/coding-agent/src/utils/clipboard-native.ts` remains a local modification, and `packages/coding-agent/src/modes/interactive/components/diff.ts` has an unresolved stash/apply conflict. This must be normalized before any trustworthy integration audit or update-path refactor.

### Child-node outcomes

The design dependencies are now resolved:
- `pi-fork-worktree-reconciliation`: the unstashed local fork delta was normalized and the `diff.ts` conflict was resolved in favor of the richer Omegon-specific renderer.
- `singular-package-runtime-boundary`: Omegon is the single installed product boundary, while `vendor/pi-mono` is a dev-only implementation source.
- `update-lifecycle-parity`: true parity requires pull/sync/build/install-or-link/verify/cache-clear steps, but should end with an explicit restart handoff instead of in-process reload.

## Decisions

### Decision: Track this as a multi-part lifecycle change before implementation

**Status:** decided
**Rationale:** The work spans fork conflict resolution, package/runtime integration audit, and update lifecycle semantics. Treating it as a tracked design and OpenSpec change prevents mixing architectural decisions with ad hoc repair and gives us a place to capture parity requirements and restart boundaries.

### Decision: Update parity means install-pi equivalence up to a verified restart handoff

**Status:** decided
**Rationale:** The parent now has enough evidence: Omegon is the single installed product boundary, vendor/pi-mono is a dev implementation source, and true update parity requires relink + binary verification but should stop at a safe restart boundary rather than trying to hot-swap the running process.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/bootstrap/index.ts` (modified) — Make `/update` perform full lifecycle parity steps for dev and installed modes, including relink/verification where applicable.
- `scripts/install-pi.sh` (modified) — Keep the script aligned with `/update` semantics or refactor shared lifecycle steps into reusable helpers.
- `bin/pi.mjs` (modified) — Preserve singular-package runtime ownership and support post-update verification of the active binary target.
- `README.md` (modified) — Remove stale split-product update language and document the singular-package update contract.
- `docs/omegon-install.md` (modified) — Align installation/update documentation with the decided singular-package ownership model.
- `extensions/bootstrap/index.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `tests/bin-where.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/singular-package-update-lifecycle/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- `/update` must be functionally equivalent to the install/link lifecycle up to a verified restart handoff.
- Package/runtime mutation must not rely on in-process hot reload as the authoritative completion path.
- Installed mode and dev mode must both verify that the active `pi` binary still resolves to Omegon after update.
- `vendor/pi-mono` remains a contributor/dev source and must not redefine the installed-product ownership model.
- /update ends at a verified restart handoff rather than in-process runtime hot swap.
- Both dev and installed update paths verify that the active `pi` binary still resolves to Omegon.

## Acceptance Criteria

### Scenarios

**Given** Omegon is installed as the single product boundary,
**When** an operator invokes `/update` in either dev mode or installed mode,
**Then** the documented lifecycle steps cover pull/install, runtime refresh, binary-target verification, and a final restart handoff without requiring a separate standalone pi package workflow.

**Given** a contributor is working from a source checkout with `vendor/pi-mono`,
**When** they compare `/update` against `./scripts/install-pi.sh`,
**Then** any remaining difference is explicitly justified as a safe restart-boundary choice rather than an accidental omission in relink, verification, or cache invalidation.

### Falsifiability

- This decision is wrong if: a freshly updated dev checkout can still leave `pi` pointing at a non-Omegon binary because `/update` omits relink or target verification.
- This decision is wrong if: installed-mode updates still depend on local `vendor/pi-mono` state or any other contributor-only workspace artifact.
- This decision is wrong if: the only way to get the new runtime after `/update` is an in-process reload path that can run against partially replaced package files.

### Constraints

- [x] All three child design dependencies are resolved and their conclusions are reflected in the parent decision.
- [x] No open questions remain at decision time.
- [x] Implementation Notes identify the files that will define/update the lifecycle boundary.
- [x] The design explicitly distinguishes `/update` from lightweight `/refresh` or reload behavior.
- [x] The design preserves Omegon as the single installed product boundary and treats `vendor/pi-mono` as dev-only implementation source.
