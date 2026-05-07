+++
id = "49ecad0b-519c-41ce-bb01-6fc418d81d7e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# runtime/package-ownership

## Requirement: Omegon-owned fork packages publish from a styrene-lab scope

The fork runtime packages required by Omegon MUST publish from a styrene-lab-controlled npm scope instead of the personal `@cwilson613/*` scope.

### Scenario: Published runtime package names align with product ownership

Given the forked runtime packages are prepared for release
When the publish workflow releases them from `styrene-lab/omegon`
Then the package names published for Omegon's runtime dependency chain use the `@styrene-lab/*` scope
And Omegon's root package depends on the `@styrene-lab/*` names rather than `@cwilson613/*`

## Requirement: Runtime resolution and extension imports stay installable after the rename

Renaming the fork packages MUST preserve Omegon's ability to resolve the coding-agent runtime and its public extension imports in both vendored-development and published-install modes.

### Scenario: Canonical Omegon runtime resolves the renamed coding-agent package

Given Omegon is running from an installed npm package
When `bin/omegon.mjs` resolves the packaged coding-agent entrypoint
Then it looks in `node_modules/@styrene-lab/pi-coding-agent/dist/cli.js`
And root extensions import public APIs from `@styrene-lab/pi-coding-agent`, `@styrene-lab/pi-ai`, and `@styrene-lab/pi-tui`

## Requirement: Release automation pins and publishes the renamed packages

The release pipeline MUST publish or pin the renamed styrene-lab-scoped fork packages before publishing Omegon itself.

### Scenario: Publish script targets styrene-lab-scoped packages

Given the release workflow invokes `scripts/publish-pi-mono.sh`
When the script checks registry state and rewrites local file dependencies
Then it publishes `@styrene-lab/pi-ai`, `@styrene-lab/pi-tui`, `@styrene-lab/pi-agent-core`, and `@styrene-lab/pi-coding-agent`
And it rewrites Omegon's root dependencies to those same names for registry publish

## Requirement: Transition guidance remains explicit for old personal-scope installs

Operators who previously encountered the personal-scope fork package names MUST receive explicit migration context rather than silent scope drift.

### Scenario: Docs explain the ownership-aligned migration

Given an operator reads Omegon installation or architecture documentation
When the docs discuss the forked pi runtime packages
Then they describe styrene-lab as the authoritative package owner
And they do not present `@cwilson613/*` as the long-term supported package boundary
