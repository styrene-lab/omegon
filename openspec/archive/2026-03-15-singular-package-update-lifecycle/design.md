# Singular package integration and full-lifecycle update parity — Design

## Architecture Decisions

### Decision: Track this as a multi-part lifecycle change before implementation

**Status:** decided
**Rationale:** The work spans fork conflict resolution, package/runtime integration audit, and update lifecycle semantics. Treating it as a tracked design and OpenSpec change prevents mixing architectural decisions with ad hoc repair and gives us a place to capture parity requirements and restart boundaries.

### Decision: Update parity means install-pi equivalence up to a verified restart handoff

**Status:** decided
**Rationale:** The parent now has enough evidence: Omegon is the single installed product boundary, vendor/pi-mono is a dev implementation source, and true update parity requires relink + binary verification but should stop at a safe restart boundary rather than trying to hot-swap the running process.

## Research Context

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

## File Changes

- `extensions/bootstrap/index.ts` (modified) — Make `/update` perform full lifecycle parity steps for dev and installed modes, including relink/verification where applicable.
- `scripts/install-pi.sh` (modified) — Keep the script aligned with `/update` semantics or refactor shared lifecycle steps into reusable helpers.
- `bin/pi.mjs` (modified) — Preserve singular-package runtime ownership and support post-update verification of the active binary target.
- `README.md` (modified) — Remove stale split-product update language and document the singular-package update contract.
- `docs/omegon-install.md` (modified) — Align installation/update documentation with the decided singular-package ownership model.

## Constraints

- `/update` must be functionally equivalent to the install/link lifecycle up to a verified restart handoff.
- Package/runtime mutation must not rely on in-process hot reload as the authoritative completion path.
- Installed mode and dev mode must both verify that the active `pi` binary still resolves to Omegon after update.
- `vendor/pi-mono` remains a contributor/dev source and must not redefine the installed-product ownership model.
