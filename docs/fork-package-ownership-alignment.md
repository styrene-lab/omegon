+++
id = "bf256138-ac9d-4062-b108-7885ab116ec8"
kind = "document"
title = "Fork package ownership alignment — move `@cwilson613/pi-*` publishing under styrene-lab control"
status = "implemented"
tags = ["npm", "publishing", "ownership", "distribution", "forks"]
aliases = ["fork-package-ownership-alignment"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "fork-package-ownership-alignment"
parent = "omegon-binary-identity"
+++

# Fork package ownership alignment — move `@cwilson613/pi-*` publishing under styrene-lab control

## Overview

Assess whether the forked pi packages should move from the `@cwilson613` npm scope and related ownership model to a styrene-lab-controlled publishing boundary so Omegon's release pipeline and trusted publishing align with the actual product owner.

## Research

### Current publish pipeline is blocked by split ownership between product repo and fork package scope

Omegon now lives in `styrene-lab/omegon`, but the forked runtime packages still publish under the personal npm scope `@cwilson613/*`. The release pipeline had to bridge submodule refs, workspace install quirks, and trusted-publishing auth, but the final blocker remains npm-side authorization for publishing `@cwilson613/pi-*` from the `styrene-lab/omegon` workflow. Even when trusted publisher settings appear correct in npm, the package boundary still spans two ownership domains: the product/release owner (`styrene-lab`) and the scoped package owner (`cwilson613`).

### Lifecycle ownership argues for moving both repo authority and npm package authority under styrene-lab

Omegon already established that executable entry, update/restart semantics, and runtime verification must live at the Omegon-controlled boundary. The same principle now applies at the package layer: if styrene-lab owns the product and release lifecycle, the forked pi packages that Omegon depends on should also publish from a styrene-lab-controlled package namespace. Keeping the fork packages under `@cwilson613` preserves a personal-authority escape hatch beneath a product that claims org-level lifecycle ownership.

## Decisions

### Decision: Fork runtime packages should migrate from `@cwilson613/*` to a styrene-lab-controlled npm scope

**Status:** decided
**Rationale:** The current split between `styrene-lab/omegon` and `@cwilson613/pi-*` undermines the same lifecycle-ownership boundary Omegon just established at the executable layer. Moving the fork packages into a styrene-lab-controlled scope aligns repo authority, package authority, trusted publishing, and release responsibility under one owner.

### Decision: The migration should rename the fork packages into a styrene-lab scope rather than only changing repository ownership

**Status:** decided
**Rationale:** Changing GitHub repo ownership alone would still leave npm publishing, trusted publishing, and dependency resolution anchored to `@cwilson613/*`. The true ownership boundary lives in the package names that Omegon installs and publishes against, so the migration must move package names and dependency references into a styrene-lab-controlled scope such as `@styrene-lab/*`.

### Decision: Legacy `@cwilson613/*` packages become compatibility debt, not the long-term release boundary

**Status:** decided
**Rationale:** Once the styrene-lab scope exists, Omegon and the forked monorepo should publish and depend on the new package names. Existing `@cwilson613/*` packages may need temporary compatibility handling or deprecation messaging, but they should no longer define the authoritative release path.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `vendor/pi-mono/packages/ai/package.json` (modified) — Rename package and dependency references into the styrene-lab scope.
- `vendor/pi-mono/packages/tui/package.json` (modified) — Rename package and dependency references into the styrene-lab scope.
- `vendor/pi-mono/packages/agent/package.json` (modified) — Rename package and dependency references into the styrene-lab scope.
- `vendor/pi-mono/packages/coding-agent/package.json` (modified) — Rename package and dependency references into the styrene-lab scope.
- `vendor/pi-mono/packages/mom/package.json` (modified) — Update internal dependencies to the new styrene-lab package names.
- `vendor/pi-mono/packages/pods/package.json` (modified) — Update internal dependencies to the new styrene-lab package names.
- `vendor/pi-mono/packages/web-ui/package.json` (modified) — Update internal dependencies to the new styrene-lab package names.
- `vendor/pi-mono/package.json` (modified) — Align workspace-root dependencies and publish assumptions with the styrene-lab scope.
- `package.json` (modified) — Change Omegon runtime dependencies from `@cwilson613/*` to the new styrene-lab scope.
- `.github/workflows/publish.yml` (modified) — Update release workflow to publish the renamed styrene-lab-scoped packages via trusted publishing.
- `scripts/publish-pi-mono.sh` (modified) — Publish and pin the renamed styrene-lab-scoped fork packages before publishing Omegon.
- `README.md` (modified) — Update installation and architecture docs to reference the styrene-lab-owned fork packages where user-facing docs mention them.
- `docs/omegon-install.md` (modified) — Document the ownership-aligned package migration and any temporary compatibility strategy.
- `bin/omegon.mjs` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `scripts/preinstall.sh` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `vendor/pi-mono` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/fork-package-ownership-alignment/tasks.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `openspec/changes/fork-package-ownership-alignment/specs/runtime/package-ownership.md` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- The migration must preserve Omegon's ability to install and run from npm in both dev and published modes.
- Trusted publishing must succeed from `styrene-lab/omegon` without depending on personal npm-token ownership for the long-term path.
- Existing users should not be stranded during the transition; compatibility or migration messaging for old `@cwilson613/*` installs must be explicit.
- The authoritative package boundary should align with the same lifecycle-ownership principle already adopted for the executable boundary.
- Preserve Omegon install/run behavior in dev and published modes while moving runtime packages to @styrene-lab/*.
- Keep @cwilson613/* as explicit legacy compatibility only during the transition.
- Allow publish workflow to use NPM_TOKEN fallback until trusted publishing is confirmed for every renamed package.

## Acceptance Criteria

### Falsifiability

- This decision is wrong if: Fail if Omegon still depends on `@cwilson613/*` packages as the intended long-term published runtime path.
- This decision is wrong if: Fail if trusted publishing for the product still requires personal-scope exceptions or personal-token ownership to complete releases.
- This decision is wrong if: Fail if package renaming happens without an explicit migration story for older installs or downstream references.
- This decision is wrong if: Fail if repository ownership changes but npm package identity remains anchored to the old personal scope.
