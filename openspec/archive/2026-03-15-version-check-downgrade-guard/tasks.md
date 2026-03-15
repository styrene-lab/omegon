# Version check downgrade guard — suppress false update prompts from older registry versions — Tasks

## 1. Version check downgrade guard — suppress false update prompts from older registry versions

<!-- specs: version-check -->

- [x] 1.1 Compare registry and running versions numerically instead of treating any mismatch as an update
- [x] 1.2 Add regression coverage for older, newer, and equal version cases with fork suffixes
## 2. Post-assess follow-up
<!-- skills: typescript -->
- [x] 2.1 Update `extensions/version-check.ts` to treat suffix-only upgrades as newer while still suppressing downgrade prompts
- [x] 2.2 Add regression coverage in `tests/version-check.test.ts` for older and newer fork-suffix versions
