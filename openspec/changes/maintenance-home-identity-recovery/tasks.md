# Implementation tasks

## 1. Establish the recovery contract
<!-- specs: home-identity -->

- [x] Inventory dependent keys and resolve explicit legacy rebind, stable evidence, and quiescence requirements; historical cause remains unknown.
- [x] Record red tests for CLI recovery and cached admission; add regression coverage for preserved denies, admission contention, interrupted recovery, tampering and replay.

## 2. Implement and verify recovery
<!-- specs: home-identity -->

- [x] Implement descriptor-bound recovery with bootstrap/domain/audit locking and immutable intent plus atomic resumable phase journal.
- [x] Add supported stable-volume continuity without weakening path/inode or descriptor race checks.
- [x] Verify unchanged-home behavior, all interrupted phases, busy guards, active transactions/fences, and refusal for unproven continuity.
- [x] Recover the observed installation through the supported command, finish catalog/extension installation, and verify real-home admission without GUI launches.

Focused evidence: `/tmp/omegon-recovery-maint-final.log` (nine unit recovery
regressions and four CLI recovery cases), `/tmp/omegon-recovery-maint-contracts-all.log`
(43 protocol tests). Final companion gates and real installation recovery are
recorded in `verification.md`.

## 3. Bound recovery descriptor usage
<!-- specs: home-identity -->

- [x] Record failing child-process tests for low-soft-limit recovery and insufficient-hard-limit refusal.
- [x] Add scoped soft-limit budgeting while retaining all protocol locks; restore limits on success and failure.
- [x] Verify successful recovery, safe hard-limit refusal, retained lock contention, and original limit restoration in isolated child processes.

Descriptor-budget evidence: `/tmp/omegon-recovery-maint-fd-red.log` (four
regressions reproduced), `/tmp/omegon-recovery-maint-fd-green.log` (four passed),
`/tmp/omegon-recovery-maint-fd-crate.log` (`just test-crate omegon-maintain`, all
73 tests passed), and `/tmp/omegon-recovery-maint-fd-clippy.log`
(`just clippy-changed`, passed). Child-process checks observed original limits
after dry-run success, contended-lock refusal, and successful apply.
