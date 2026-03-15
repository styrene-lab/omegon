# update-lifecycle — Delta Spec

## ADDED Requirements

### Requirement: /update performs the singular-package lifecycle

Omegon's `/update` command must reflect the singular-package ownership model rather than a split wrapper/core mental model.

#### Scenario: Dev-mode update ends with verified restart handoff
Given Omegon is running from a source checkout with `vendor/pi-mono`
When the operator runs `/update`
Then the update flow performs pull, submodule sync, build, dependency refresh, and binary-target verification steps required for the active Omegon install
And it ends by instructing the operator to restart instead of relying on in-process reload as the authoritative completion path

#### Scenario: Installed-mode update verifies the active binary target
Given Omegon is installed globally from npm
When the operator runs `/update`
Then the update flow installs the new package version
And verifies that the active `pi` command still resolves to Omegon
And ends with the same restart-handoff contract used by dev mode

### Requirement: /refresh remains lightweight

`/refresh` is a cache-clear and extension reload convenience path, not a replacement for package/runtime mutation.

#### Scenario: Refresh is not described as full update parity
Given an operator reads the update documentation or command messaging
When `/refresh` is mentioned
Then it is described as a lightweight cache-refresh path
And it is not described as equivalent to `/update` or install/relink lifecycle mutation
