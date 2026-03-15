# Version check downgrade guard — suppress false update prompts from older registry versions — Design

## Approach

Apply the same suffix-aware comparison rule to both update-notification paths in this repo.

- Parse all numeric segments from version strings such as `0.58.1-cwilson613.1`
- Compare segments lexicographically so `0.58.1-cwilson613.2` sorts above `0.58.1-cwilson613.1`
- Suppress notifications when the registry version is equal to or older than the running build
- Keep the fix narrow to the vendored pi interactive startup path and Omegon's extension-level version checker

## Files

- `vendor/pi-mono/packages/coding-agent/src/modes/interactive/interactive-mode.ts`
- `vendor/pi-mono/packages/coding-agent/test/interactive-mode-status.test.ts`
- `extensions/version-check.ts`
- `tests/version-check.test.ts`
