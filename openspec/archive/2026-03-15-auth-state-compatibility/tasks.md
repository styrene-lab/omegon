+++
id = "ac18d485-7a05-4a8b-8811-d32ec98a039a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auth state compatibility — preserve existing pi/Claude Code logins in Omegon — Tasks

## 1. Runtime state/resource split in `bin/omegon.mjs`
<!-- specs: runtime/auth-state -->
- [x] 1.1 Stop forcing `PI_CODING_AGENT_DIR` to `omegonRoot` during normal installed execution.
- [x] 1.2 Report both the packaged Omegon root and the resolved persistent state directory in `omegon --where` metadata.
- [x] 1.3 Inject Omegon-packaged extension/skill/prompt paths at startup so packaged resources still load when mutable state stays under `~/.pi/agent`.
- [x] 1.4 Migrate legacy package-root `auth.json` / `settings.json` into the shared state directory when the shared copy is absent.
- [x] 1.5 Preserve explicit `PI_CODING_AGENT_DIR` overrides instead of replacing them.

## 2. Verification and regression coverage
<!-- specs: runtime/auth-state -->
- [x] 2.1 Update bootstrap verification helpers to understand the split between `omegonRoot` and persistent state dir.
- [x] 2.2 Expand `tests/bin-where.test.ts` to cover default shared-state behavior, explicit override behavior, and legacy migration behavior.
- [x] 2.3 Expand `extensions/bootstrap/index.test.ts` to validate the new `--where` metadata shape and verification expectations.

## 3. Constraint reconciliation
<!-- specs: runtime/auth-state -->
- [x] 3.1 Verify the happy path preserves existing `~/.pi/agent/auth.json` logins from Claude Code/pi.
- [x] 3.2 Verify installed Omegon still loads its bundled resources without requiring user settings edits.
- [x] 3.3 Verify users who ran the regressed package-root state build are migrated forward safely.
