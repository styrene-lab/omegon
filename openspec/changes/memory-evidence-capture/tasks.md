## 1. Structured committed episodes
<!-- specs: memory/formation -->

- [x] Reuse canonical semantic test fixtures and validate goal/correction capture, assistant-report attribution, and a denied tool outcome without treating it as success.
- [x] Wire the deferred session binding through setup and capture bounded default-projection evidence from validated replay; reject missing or replaced sources.
- [x] Retain canonical session ownership and exclude uncommitted advisory strings, thinking content, and restricted continuity from formation input.
- [ ] Extend evaluation to multi-session workflow outcomes and evidence-eviction checkpoints.

## 2. Typed extraction and durable work
<!-- specs: memory/formation -->

- [x] Validate classified candidates with fake extraction; reject malformed siblings, fabricated references, and supplied authority fields.
- [x] Persist a typed source frontier and pending episode before inference; complete candidates atomically with unchanged evidence, FTS updates, vector invalidation, and replay receipts.
- [x] Verify reopen and JSONL round-trip, pending-to-complete import, corruption errors, rejected completion, and rollback after receipt failure.
- [ ] Implement interval/pre-eviction checkpoints, durable scheduling, and automatic restart recovery of pending extraction.

## 3. Bounded lifecycle and verification
<!-- specs: memory/formation -->

- [ ] RED: Add saturation, cancellation, and evidence-eviction checkpoint tests with controlled scheduling rather than sleeps.
- [ ] GREEN: Implement backpressure and recoverable managed shutdown behavior.
- [x] Verify bounded Unicode excerpts and candidate payloads, controlled inference timeout, and durable source preservation after task cancellation.
- [ ] Verify migration/reopen and main-crate integration; map scenarios to tests and record red/green results.
- [ ] Run applicable landing gates from ../../memory-modernization.md and validate this change.
- [x] Complete the Wave 3 slice gate in verification-wave-3.md; keep the remaining checkpoint/queue tasks open.
