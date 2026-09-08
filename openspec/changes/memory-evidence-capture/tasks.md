## 1. Structured committed episodes
<!-- specs: memory/formation -->

- [ ] RED: Add synthetic multi-session fixtures for a middle-of-session correction, failed attempt, verification, and unresolved work.
- [ ] GREEN: Wire canonical evidence references from loop/session finalization and form substantive episodes in the memory domain.
- [ ] REFACTOR: Share evidence access with existing session-log ownership rather than copying a second transcript.

## 2. Typed extraction and durable work
<!-- specs: memory/formation -->

- [ ] RED: Add fake-extractor cases for mixed categories, malformed output, fabricated references, restart, and partial-batch replay.
- [ ] GREEN: Implement structured candidate validation, checkpoint watermarks, persisted extraction batches, and replay-safe scheduling.
- [ ] REFACTOR: Share pending-work and mutation identity handling with managed memory conventions.

## 3. Bounded lifecycle and verification
<!-- specs: memory/formation -->

- [ ] RED: Add saturation, cancellation, and evidence-eviction checkpoint tests with controlled scheduling rather than sleeps.
- [ ] GREEN: Implement backpressure and recoverable managed shutdown behavior.
- [ ] Verify migration/reopen and main-crate integration; map scenarios to tests and record red/green results.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
