# Unified request preparation tasks

Dependencies: group 1 establishes the common boundary; groups 2 and 3 migrate
the remaining flows; group 4 verifies the complete approved scope. IDs are stable
within this change. Lifecycle tool registration is unavailable in this session.

## 1. Normal and repair preparation
<!-- specs: inference/preparation -->

- [x] 1.1 Add private immutable request inputs with explicit tool policy, capability validation, and evidence-before-dispatch ordering in model_request.rs.
- [x] 1.2 Consolidate initial, context-overflow, and history-repair capture/dispatch construction while preserving request replacement, tool lineage, and cancellation owners.
- [x] 1.3 Verify exact inputs, ownership rejection, capability/evidence failure, selected/serving identity, retry identity, and existing tool admission/revocation fixtures.

## 2. Compaction preparation
<!-- specs: inference/preparation -->

- [x] 2.1 Adapt turn, idle, and compatibility compact_loop_route to the no-tools envelope and retain their evidence owners.
- [x] 2.2 Verify authority overflow rejection and UTF-8-safe compatibility truncation without changing context selection.
- [x] 2.3 Verify Done/nonempty commitment and EOF, timeout, provider-error rejection with fake bridges and authority fixtures.

## 3. Bounded auxiliary extraction
<!-- specs: inference/preparation -->

- [x] 3.1 Adapt quick_completion_bounded to sessionless no-tools preparation with the byte budget consumed by its collector.
- [x] 3.2 Verify step evidence, failed evidence writes, output overflow, explicit Done, EOF/error, and unchanged unbounded helper compatibility.
- [x] 3.3 Verify memory deadline, parser rejection, cancellation, and stale-generation publication behavior through existing owner tests.

## 4. Complete-scope review
<!-- specs: inference/preparation -->

- [x] 4.1 Update Unreleased, design decisions, and scenario-to-test verification evidence for all included flows.
- [x] 4.2 Run focused tests, just test-crate omegon, just clippy-changed, narrow formatting, diff check, and OpenSpec structural validation using the pinned environment.
- [x] 4.3 Review the implementation and record findings, remaining limitations, and exact reviewable git state without archival or publication.
