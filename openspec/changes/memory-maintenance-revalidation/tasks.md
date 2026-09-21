## 1. 7A — Freeze policy, DTOs, and ownership before migration
<!-- specs: memory/maintenance -->

- [ ] Record the accepted integrated Wave 6 source-independence/correction contract, DTO version, commit, and verification reference before correction integration; mark the dependency pending until then.
- [ ] Freeze per-kind policy for durable invariants, temporary guidance, episodic observations, and legacy/unknown facts; define classification origin, review conditions, evidence confidence, usage, validity, and retention.
- [ ] Freeze observed/checked/verified timestamp meanings, unknown legacy semantics, and the rule that confidence 1.0, usage counts, source inspection, and index freshness do not imply verification.
- [ ] Assign one exclusive owner for types.rs, schema, backend mutations, and shared DTOs; freeze the whole-plan atomic write-set and field-by-field transport matrix before migration.
- [ ] Characterize existing no-reinforcement reads, provenance inspection, applicability, transport_history, vault idempotency, and indexing tests; record existing pass evidence rather than inventing RED failures.
- [ ] Record current dormancy repeated-apply behavior, duplicate Store reinforcement, vault related_facts/ReinforceFactOnce, reinforce_references configuration, and facts_reinforced reports; freeze compatibility changes with Wave 6 ownership.
- [ ] Define new behavior tests for old-supported selection, explicit memory kinds, checked-versus-verified outcomes, and temporary-guidance expiration; record genuine behavioral RED results when implementation starts.

## 2. 7B — Bounded source snapshots and scheduler contract
<!-- specs: memory/maintenance -->

- [ ] Freeze triggers, cadence, stable source/work/attempt IDs, fact fanout pages and watermarks, fairness, queue capacity, concurrency, backoff, deadlines, cancellation, and reopen policy with concrete limits.
- [ ] Preserve vault file/snapshot and lifecycle artifact limits plus managed page bounds, or document explicit tighter limits; state cooperative filesystem cancellation accurately.
- [ ] Add synthetic source fixtures for changed, changed-back, missing, readable-unverified, unsupported, denied, escape, oversized, truncated, cancelled, exhausted-budget, and provider-outage outcomes.
- [ ] Implement host-owned secure source snapshots against frozen DTOs with exact source generation and fact preconditions; keep inspection distinct from revalidation and embedding indexing.
- [ ] Verify incomplete-read outcomes preserve evidence and checked/verified times, never produce negative findings, and report bounded uncertainty and retry classification.
- [ ] Verify fair paged fanout, concurrent changes, late arrivals, capacity deferral, cancellation boundaries, and restart cursors using injected clocks and source snapshots.

## 3. 7C — Atomic domain state and receipt-backed maintenance
<!-- specs: memory/maintenance -->

- [ ] Extend existing maintenance.rs DormancyPlan scaffolding with canonical payload, operation identity, policy version, reasons, FactPrecondition, source preconditions, and complete write-set.
- [ ] Reuse MemoryMutation::TransitionFacts and existing backend receipt admission; extend the existing owner only as needed for atomic evidence, lifecycle, queue completion, and selection invalidation.
- [ ] Add stale fact/source, multi-fact rollback, changed-payload identity, exact replay, and replay-after-source-removal tests for both backends; check receipt before fresh source/version reads.
- [ ] Implement whole-plan atomic admission and original-outcome replay across reopen; treat independent bounded pages as separately identified operations.
- [ ] Implement schema migration only after 7A freeze, allocating its number from the integrated schema rather than guessing after baseline schema 14; verify rollback and legacy unknowns.

## 4. 7D — Managed runtime and operator integration
<!-- specs: memory/maintenance -->

- [ ] Integrate durable scheduler requests through core/crates/omegon/src/memory_service.rs; keep source reads and provider calls host-owned outside domain transactions.
- [ ] Gate correction on the accepted integrated Wave 6 contract; consume its source-independence and reconciliation outcomes without implementing competing admission policy.
- [ ] Implement configured triggers, cadence, bounded retry/backoff, cancellation, capacity reporting, and resume with stable work identity.
- [ ] Register review/apply/status/cancel behavior through existing command ownership and CommandDefinition metadata for applicable TUI/CLI/ACP surfaces.
- [ ] Update vault reference and duplicate-write attention behavior under the frozen mapping, including reports, configuration compatibility, and operator guidance about reinforcement/resetting decay.
- [ ] Verify provider outage leaves storage and lexical reads available, diagnostics omit source contents, and runtime shutdown/cancellation does not admit stale completions.

## 5. 7E — Coordinated selection, retained history, and transport
<!-- specs: memory/maintenance -->

- [ ] Change decay.rs effective_confidence, ambient_score floor policy, maintenance dormancy, and selection_cache.rs confidence_floor_deadline use together; preserve or explicitly amend cache rank-freshness bounds.
- [ ] Verify cold/cached parity for old supported facts, validity crossings, unknown legacy facts, support changes, and repeated attention without evidence changes.
- [ ] Add explicit all-status retained-history/as-of semantics across types, backend filters, lexical/vector lookup, graph, and public adapters while preserving SearchIntent::Historical archive population.
- [ ] Verify Active-but-expired retrieval without archive transitions, inclusive-from/exclusive-until scope, omitted-time unknowns, pending/unverified labels, and filtering before truncation.
- [ ] Implement the field-by-field portable/legacy/local policy for confidence, reinforcement, last_accessed, provenance, applicability, lifecycle metadata, new evidence history, and new local attention.
- [ ] Verify imported verification remains attributed, legacy absence does not erase history, and import never overwrites destination queue/receipt/progress/attention state.
- [ ] Run both-backend modern/legacy JSONL round trips, migration failure/reopen, and bidirectional vault idempotency and path-boundary fixtures.

## 6. 7F — Quality, landing evidence, and slice handoffs
<!-- specs: memory/maintenance -->

- [ ] Build a fresh synthetic development/held-out corpus contrasting stale guidance and old valid knowledge with source-failure, usage-only, changed-back, false-correction, and false-retirement negative cases.
- [ ] Freeze numerical per-kind quality thresholds, corpus IDs, policy version, and execution budgets before held-out evaluation; keep expected labels outside writer/retriever input.
- [ ] Report stale-guidance precision, old-supported retention, false verification/retirement/correction, history coverage, queue fairness, and bounded execution cost against frozen thresholds.
- [ ] Obtain fresh bounded authorization before live-model evaluation; do not reuse the Wave 5 budget as permission or its results as Wave 7 quality evidence.
- [ ] Record scenario-to-test mappings, characterization versus genuine RED/GREEN evidence, exact commands, both-backend parity, and runtime integration results.
- [ ] Run applicable memory and host landing gates from ../../memory-modernization.md, including reopen/atomicity and public adapter checks; run named OpenSpec validation and whitespace checks.
- [ ] Record accepted Wave 6 integration evidence and publish the accepted evidence-versus-attention/validity slice reference for Wave 8 without requiring unrelated scheduler completion.
