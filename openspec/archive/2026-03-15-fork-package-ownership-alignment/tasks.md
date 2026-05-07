+++
id = "3e557812-bde3-4d1f-9809-e6f895ae9f96"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fork package ownership alignment — move `@cwilson613/pi-*` publishing under styrene-lab control — Tasks

## 1. Rename fork runtime package identities <!-- specs: runtime/package-ownership -->

- [x] 1.1 Rename the published runtime packages from `@cwilson613/*` to `@styrene-lab/*` in:
  - `vendor/pi-mono/packages/ai/package.json`
  - `vendor/pi-mono/packages/tui/package.json`
  - `vendor/pi-mono/packages/agent/package.json`
  - `vendor/pi-mono/packages/coding-agent/package.json`
- [x] 1.2 Update internal workspace dependencies to the new styrene-lab package names in:
  - `vendor/pi-mono/packages/mom/package.json`
  - `vendor/pi-mono/packages/pods/package.json`
  - `vendor/pi-mono/packages/web-ui/package.json`
  - package source/test/example imports across `vendor/pi-mono/packages/**`
- [x] 1.3 Align vendored workspace bootstrap assumptions in `vendor/pi-mono/package.json` with the renamed styrene-lab package boundary.
- [x] 1.4 Swap the coding-agent optional clipboard dependency back to an installable package (`@mariozechner/clipboard`) so workspace installs succeed after the scope migration.

## 2. Move Omegon runtime and publish flow to the new package boundary <!-- specs: runtime/package-ownership -->

- [x] 2.1 Change Omegon runtime dependencies in `package.json` from `@cwilson613/*` to `@styrene-lab/*`.
- [x] 2.2 Update `bin/omegon.mjs` and bootstrap verification fixtures/tests to resolve `node_modules/@styrene-lab/pi-coding-agent` in installed mode.
- [x] 2.3 Update `scripts/publish-pi-mono.sh` so the release pipeline publishes and pins:
  - `@styrene-lab/pi-ai`
  - `@styrene-lab/pi-tui`
  - `@styrene-lab/pi-agent-core`
  - `@styrene-lab/pi-coding-agent`
- [x] 2.4 Update root extension imports, helper package manifests, and supporting scripts to compile against the styrene-lab public package names.

## 3. Document the ownership-aligned migration <!-- specs: runtime/package-ownership -->

- [x] 3.1 Update `README.md` to describe the styrene-lab-scoped fork packages as Omegon's authoritative runtime package boundary.
- [x] 3.2 Update `docs/omegon-install.md` to describe the migration from `@cwilson613/*` to `@styrene-lab/*` and the expected installed-mode resolution path.
- [x] 3.3 Refresh project docs/skills/scripts that referenced the old personal package scope so contributor guidance matches the new ownership model.

## 4. Validate the migration <!-- specs: runtime/package-ownership -->

- [x] 4.1 Rebuild the vendored runtime packages after the rename:
  - `cd vendor/pi-mono && npm --prefix packages/tui run build && npm --prefix packages/ai run build && npm --prefix packages/agent run build && npm --prefix packages/coding-agent run build`
- [x] 4.2 Verify Omegon still typechecks after the dependency rename:
  - `npm run typecheck`
- [x] 4.3 Verify Omegon executable-boundary tests still pass:
  - `npx tsx --test extensions/bootstrap/index.test.ts tests/bin-where.test.ts`
- [x] 4.4 Verify the vendored coding-agent update/version-check path still passes its targeted regression suite:
  - `cd vendor/pi-mono && npx vitest --run packages/coding-agent/test/interactive-mode-status.test.ts`
