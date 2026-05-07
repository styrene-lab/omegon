+++
id = "743e85b4-21d3-4858-97f9-8744a6fbe000"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Fork package ownership alignment — move `@cwilson613/pi-*` publishing under styrene-lab control — Design Spec

## Scenarios

### Scenario 1 — Fork packages publish from a styrene-lab-controlled namespace

Given the forked pi runtime packages are prepared for release
When the release workflow publishes them
Then the published package names live in a styrene-lab-controlled npm scope
And Omegon no longer depends on `@cwilson613/*` as the authoritative release boundary

### Scenario 2 — Omegon release pipeline aligns repo and package ownership

Given `styrene-lab/omegon` runs the publish workflow
When fork packages and Omegon are released
Then trusted publishing succeeds without relying on a personal `cwilson613` package-ownership boundary
And the release pipeline treats styrene-lab as the single owning authority

### Scenario 3 — Transition does not strand existing installs

Given users or automation may still reference older `@cwilson613/*` package names
When the migration lands
Then a deliberate compatibility or migration strategy exists
And operators are not left with a silent or ambiguous upgrade failure

## Falsifiability

- Fail if Omegon still depends on `@cwilson613/*` packages as the intended long-term published runtime path.
- Fail if trusted publishing for the product still requires personal-scope exceptions or personal-token ownership to complete releases.
- Fail if package renaming happens without an explicit migration story for older installs or downstream references.
- Fail if repository ownership changes but npm package identity remains anchored to the old personal scope.

## Constraints

- Preserve Omegon's ability to install and run from npm in both dev and published modes.
- The authoritative package boundary must align with the same lifecycle-ownership principle already adopted for the executable boundary.
- Compatibility debt may remain temporarily, but the long-term release boundary must move under styrene-lab control.
