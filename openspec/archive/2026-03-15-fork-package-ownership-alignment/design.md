+++
id = "190015ab-d6a6-4b3b-baba-296d555e9a71"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fork package ownership alignment — move `@cwilson613/pi-*` publishing under styrene-lab control — Design

## Architecture Decisions

### Decision: Fork runtime packages should migrate from `@cwilson613/*` to a styrene-lab-controlled npm scope

**Status:** decided
**Rationale:** The current split between `styrene-lab/omegon` and `@cwilson613/pi-*` undermines the same lifecycle-ownership boundary Omegon just established at the executable layer. Moving the fork packages into a styrene-lab-controlled scope aligns repo authority, package authority, trusted publishing, and release responsibility under one owner.

### Decision: The migration should rename the fork packages into a styrene-lab scope rather than only changing repository ownership

**Status:** decided
**Rationale:** Changing GitHub repo ownership alone would still leave npm publishing, trusted publishing, and dependency resolution anchored to `@cwilson613/*`. The true ownership boundary lives in the package names that Omegon installs and publishes against, so the migration must move package names and dependency references into a styrene-lab-controlled scope such as `@styrene-lab/*`.

### Decision: Legacy `@cwilson613/*` packages become compatibility debt, not the long-term release boundary

**Status:** decided
**Rationale:** Once the styrene-lab scope exists, Omegon and the forked monorepo should publish and depend on the new package names. Existing `@cwilson613/*` packages may need temporary compatibility handling or deprecation messaging, but they should no longer define the authoritative release path.

## Research Context

### Current publish pipeline is blocked by split ownership between product repo and fork package scope

Omegon now lives in `styrene-lab/omegon`, but the forked runtime packages still publish under the personal npm scope `@cwilson613/*`. The release pipeline had to bridge submodule refs, workspace install quirks, and trusted-publishing auth, but the final blocker remains npm-side authorization for publishing `@cwilson613/pi-*` from the `styrene-lab/omegon` workflow. Even when trusted publisher settings appear correct in npm, the package boundary still spans two ownership domains: the product/release owner (`styrene-lab`) and the scoped package owner (`cwilson613`).

### Lifecycle ownership argues for moving both repo authority and npm package authority under styrene-lab

Omegon already established that executable entry, update/restart semantics, and runtime verification must live at the Omegon-controlled boundary. The same principle now applies at the package layer: if styrene-lab owns the product and release lifecycle, the forked pi packages that Omegon depends on should also publish from a styrene-lab-controlled package namespace. Keeping the fork packages under `@cwilson613` preserves a personal-authority escape hatch beneath a product that claims org-level lifecycle ownership.

## File Changes

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

## Constraints

- The migration must preserve Omegon's ability to install and run from npm in both dev and published modes.
- Trusted publishing must succeed from `styrene-lab/omegon` without depending on personal npm-token ownership for the long-term path.
- Existing users should not be stranded during the transition; compatibility or migration messaging for old `@cwilson613/*` installs must be explicit.
- The authoritative package boundary should align with the same lifecycle-ownership principle already adopted for the executable boundary.
