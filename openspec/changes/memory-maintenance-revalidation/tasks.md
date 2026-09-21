## 1. Separate knowledge and attention semantics
<!-- specs: memory/maintenance -->

- [ ] RED: Add fake-clock cases for old valid invariants, repeated reads/references, and expired temporary guidance.
- [ ] GREEN: Implement separate evidence, freshness, salience, validity, and retention policy in decay/maintenance owners.
- [ ] REFACTOR: Remove implicit confidence mutations from retrieval, rendering, and transport paths.

## 2. Source revalidation and planned transitions
<!-- specs: memory/maintenance -->

- [ ] RED: Add changed-source, transient failure, vault path escape, stale-plan, and replay tests.
- [ ] GREEN: Implement bounded source-linked revalidation and versioned maintenance plans with observable reasons.
- [ ] REFACTOR: Share correction admission with reconciliation and use existing command/status registration.

## 3. Migration and verification
<!-- specs: memory/maintenance -->

- [ ] RED: Add legacy high-reinforcement migration and portable/local metadata round-trip fixtures.
- [ ] GREEN: Implement explicit legacy interpretation and compatible persistence/transport.
- [ ] Verify both backends, reopen, vault idempotency, and historical retrieval; record scenario mappings and red/green results.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
