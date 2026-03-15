# Singular package integration and full-lifecycle update parity — Tasks

## 1. pi-mono local fork worktree reconciliation

- [x] 1.1 Preserve the richer local `vendor/pi-mono` diff renderer during unstash reconciliation and clear the merge conflict in `packages/coding-agent/src/modes/interactive/components/diff.ts`
- [x] 1.2 Keep the surviving local fork deltas staged as intentional local workspace work (`agent-session.ts`, `auth-storage.ts`, `tool-execution.ts`, `clipboard-native.ts`, `anthropic.ts`) rather than accidental stash residue

## 2. Singular package runtime boundary and ownership

- [x] 2.1 Update `README.md` to describe Omegon as the single installed product boundary and remove stale split-product update guidance
- [x] 2.2 Update `docs/omegon-install.md` so install/update semantics clearly distinguish dev-only `vendor/pi-mono` source usage from installed-mode runtime ownership
- [x] 2.3 Verify `bin/pi.mjs` still encodes vendor-first dev resolution with `node_modules` fallback for installed mode, and document any verification behavior needed by `/update`

## 3. Update lifecycle parity and restart boundary

<!-- specs: update-lifecycle -->

- [x] 3.1 Refactor `extensions/bootstrap/index.ts` so dev-mode `/update` performs the full lifecycle sequence: pull, submodule sync, build, dependency refresh, relink-or-equivalent verification, cache clear, and explicit restart handoff
- [x] 3.2 Refactor `extensions/bootstrap/index.ts` so installed-mode `/update` verifies the active `pi` target after global install and ends with the same restart-handoff contract
- [x] 3.3 Keep `/refresh` as the lightweight cache-clear/reload path and ensure docs/messages no longer imply it is equivalent to package/runtime replacement
- [x] 3.4 Align `scripts/install-pi.sh` with the `/update` implementation so both paths share the same lifecycle contract and post-update verification expectations

## 4. Verification and lifecycle reconciliation

- [x] 4.1 Add or update targeted coverage for the new `/update` lifecycle semantics and binary-target verification helpers
- [x] 4.2 Reconcile implementation docs/spec artifacts, then run `/assess spec singular-package-update-lifecycle`
