# Singular package integration and full-lifecycle update parity — Design Spec

> This spec defines acceptance criteria for the design phase.
> Add Given/When/Then scenarios that must be true before marking this node 'decided'.

## Scenarios

### Scenario: /update matches the singular-package lifecycle contract

Given Omegon is the single installed product boundary
When an operator invokes `/update` in dev mode or installed mode
Then the lifecycle definition includes the required install/link, verification, cache invalidation, and restart-handoff steps
And it does not require a separate standalone pi package workflow

### Scenario: restart boundary is an explicit design choice, not missing work

Given a contributor compares `/update` with `./scripts/install-pi.sh`
When they inspect the decided design
Then any remaining difference is explicitly justified as a safe restart-boundary choice
And not as an accidental omission in relink, verification, or cache invalidation

## Falsifiability

- This decision is wrong if a freshly updated dev checkout can still leave `pi` pointing at a non-Omegon binary because `/update` omits relink or target verification.
- This decision is wrong if installed-mode updates still depend on local `vendor/pi-mono` state or any other contributor-only workspace artifact.
- This decision is wrong if the only way to get the new runtime after `/update` is an in-process reload path that can run against partially replaced package files.

## Constraints

- [x] All three child design dependencies are resolved and their conclusions are reflected in the parent decision.
- [x] No open questions remain at decision time.
- [x] Implementation Notes identify the files that will define/update the lifecycle boundary.
- [x] The design explicitly distinguishes `/update` from lightweight `/refresh` or reload behavior.
- [x] The design preserves Omegon as the single installed product boundary and treats `vendor/pi-mono` as dev-only implementation source.
