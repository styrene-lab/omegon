## 1. Structured committed episodes
<!-- specs: memory/formation -->

- [x] Reuse canonical semantic test fixtures and validate goal/correction capture, assistant-report attribution, and a denied tool outcome without treating it as success.
- [x] Wire the deferred session binding through setup and capture bounded default-projection evidence from validated replay; reject missing or replaced sources.
- [x] Retain canonical session ownership and exclude uncommitted advisory strings, thinking content, and restricted continuity from formation input.
- [x] Extend evaluation to multi-session workflow outcomes and evidence-eviction checkpoints.

## 2. Typed extraction and durable work
<!-- specs: memory/formation -->

- [x] Validate classified candidates with fake extraction; reject malformed siblings, fabricated references, and supplied authority fields.
- [x] Persist a typed source frontier and pending episode before inference; complete candidates atomically with unchanged evidence, FTS updates, vector invalidation, and replay receipts.
- [x] Verify reopen and JSONL round-trip, pending-to-complete import, corruption errors, rejected completion, and rollback after receipt failure.
- [x] Complete incremental evidence coverage; recovery, interval snapshots, and awaited pre-compaction capture are tracked in sections 5–10.

## 3. Bounded lifecycle and verification
<!-- specs: memory/formation -->

- [x] Add saturation, cancellation, and evidence-eviction checkpoint tests with controlled scheduling; record initial failures and review-found defects in verification-wave-5-closure.md.
- [x] GREEN: Implement backpressure and recoverable managed shutdown behavior.
- [x] Verify bounded Unicode excerpts and candidate payloads, controlled inference timeout, and durable source preservation after task cancellation.
- [x] Verify migration/reopen and main-crate integration; map scenarios to tests and record red/green results.
- [x] Run applicable landing gates from ../../memory-modernization.md and validate this change.
- [x] Complete the Wave 3 slice gate in verification-wave-3.md; sections 5–10 complete its remaining checkpoint/queue contracts.

## 4. Post-acceptance adversarial hardening
<!-- specs: memory/formation -->

- [x] RED/GREEN: Enforce extraction byte limits during streaming and require terminal Done.
- [x] RED/GREEN: Make capture identity and payload stable across advisory changes and mind scopes.
- [x] RED/GREEN: Stop assistant excerpts at unavailable chunks and enforce source identity consistency.
- [x] Verify legacy invalid-model diagnostics preserve valid source evidence.
- [x] Complete final rechecks and handoff in adversarial-review-wave-3.md before Wave 4.

## 5. Wave 5C bounded startup recovery
<!-- specs: memory/formation -->

- [x] RED: Reproduce persisted pending evidence remaining unprocessed after a fresh feature startup.
- [x] GREEN: Add bounded mind/model-filtered pending inventory and startup extraction using retained evidence.
- [x] Verify SQLite reopen, completed-record exclusion, overflow preservation, corrupt inventory errors, and backend parity.
- [x] Verify fresh-owner repeat behavior and managed shutdown cancellation without sleeps.
- [x] Complete landing gates, same-executor review, and verification-wave-5-recovery.md.

## 6. Wave 5C continuous pending-work scheduling
<!-- specs: memory/formation -->

- [x] RED: Reproduce the ninth pending checkpoint requiring another startup after the initial eight-record pass.
- [x] GREEN: Add serial bounded passes, idle polling, capped failure backoff, and read-only scheduler observations through hosted memory_query.
- [x] Verify paused-clock idle discovery, backlog progression, timeout cleanup, backoff/reset, and cancellation during waits.
- [x] Verify SQLite-backed overflow progression, feature-drop cancellation, and startup/shutdown regressions.
- [x] Complete final gates and same-executor review in verification-wave-5-scheduler.md.

## 7. Wave 5C interval evidence snapshots
<!-- specs: memory/formation -->

- [x] RED: Reproduce missing persisted evidence after eight turns before SessionEnd.
- [x] GREEN: Persist bounded committed snapshots through a single owned interval worker and existing capture receipts.
- [x] Verify configured pending capture without inference, repeated-source replay, SQLite reopen, and unavailable-source rejection.
- [x] Verify coalesced due pressure, read-only observations, and managed shutdown joining the worker slot.
- [x] Complete final gates and same-executor review in verification-wave-5-checkpoints.md.

## 8. Wave 5C awaited pre-compaction checkpoints
<!-- specs: memory/formation -->

- [x] RED: Reproduce absent durable acknowledgment before interval/session finalization through the default optional hook.
- [x] GREEN: Add the compatible Feature hook and invoke it before pressure, overflow, requested/manual compaction, and aggressive decay.
- [x] Verify persistence acknowledgment, read-only outcome reporting, occupied-slot degradation, and unchanged interval behavior.
- [x] Verify awaited ordering, published-feature admission, shared timeout/cancellation, optional absence, and outcome serialization.
- [x] Complete shared-contract/reverse-dependent gates and same-executor review in verification-wave-5-pre-eviction.md.

## 9. Wave 5C durable coverage contract
<!-- specs: memory/formation -->

- [x] RED: Demonstrate silent loss of legacy coverage declarations and rejection of version-2 coverage envelopes.
- [x] GREEN: Add versioned explicit coverage with range validation and immutable completion semantics.
- [x] Verify backend parity, receipt replay, SQLite reopen, JSONL completion, and legacy unknown coverage.
- [x] Complete landing gates and same-executor review in verification-wave-5-coverage.md.
- [x] Integrate bounded incremental capture and durable watermark discovery in the host after the domain contract lands.

## 10. Wave 5 completion: local range capture
<!-- specs: memory/formation -->

- [x] RED: Reproduce unsupported local cursor recovery after a page write through both storage backends.
- [x] GREEN: Commit contiguous capture pages and local cursor receipts atomically; exclude imported declarations from resume progress.
- [x] Verify receipt-failure rollback, reopen, namespace separation, and atomic selected candidate batch replay.
- [x] Verify bounded host overflow, startup resume, finalization replay, and cancellation/backpressure with canonical source fixtures.
- [x] Complete combined landing gates, independent review, and final scenario-to-evidence closure before archival.
