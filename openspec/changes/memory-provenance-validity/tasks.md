## 1. Evidence and applicability vocabulary
<!-- specs: memory/provenance -->

- [ ] Resolve canonical workspace/revision descriptors and evidence retention behavior before freezing schema fields.
- [ ] RED: Add observed-versus-inferred, imported-authority, platform applicability, historical validity, and unavailable-source tests.
- [ ] GREEN: Implement domain evidence/applicability types and retrieval inspection using host-owned source identities.
- [ ] REFACTOR: Keep authority interpretation out of renderers and transport adapters.

## 2. Lifecycle admission and atomic corrections
<!-- specs: memory/lifecycle -->

- [ ] RED: Exercise artifact retention, explicit constraints, archived specs, pending inferred summaries, replay, and version-conflict rollback via lifecycle ingestion.
- [ ] GREEN: Persist authority and references, enforce the existing confirmation policy, and apply supersedes through atomic mutation contracts.
- [ ] REFACTOR: Share admission semantics with ordinary domain callers without duplicating the lifecycle engine.

## 3. Persistence and verification
<!-- specs: memory/provenance, memory/lifecycle -->

- [ ] RED: Add supported legacy DB and JSONL fixtures plus failed-migration, reopen, and vault round-trip cases.
- [ ] GREEN: Implement migration and schema-contract updates preserving legacy unknowns and operational metadata.
- [ ] Verify vault idempotency/path boundaries and both backends; record scenario mappings and red/green outcomes.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
