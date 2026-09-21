# Post-acceptance adversarial review: Wave 3

## Scope and verdict

Reviewed the implementation accepted at `f90e793e` and its episode retrieval
dependencies before starting Wave 4. Branch: `fix/memory-wave3-adversarial`.
This is a separate same-executor review; no independent reviewer is claimed.

Six concrete findings were reproduced and addressed in `987272da`. Final
compatibility checks, the affected-crate gate, and Clippy passed. No blocking
finding remains in this review's scope. Wave 4 is unblocked and remains planned.

## Findings and disposition

### F1 — High: output limits and completion were enforced too late

`ModelExtractor` used the general quick-completion collector, which accumulated
all deltas before the 64 KiB parser limit applied. It also accepted EOF without
Done and waited for EOF after Done. This allowed oversized retained output,
incomplete responses classified as complete, and unnecessary extraction timeouts.

Fixed with a bounded host collector used by memory: check bytes before appending,
reject EOF without Done, and finish/drop the receiver at Done. Existing callers of
the general quick-completion API retain their compatibility behavior.

Evidence: `adversarial_bounded_completion_rejects_oversized_utf8`,
`adversarial_bounded_completion_rejects_eof_without_done`, and
`adversarial_bounded_completion_closes_at_done`. All three had behavioral red results.
The overflow case holds the producer open to prove rejection does not wait for EOF.

### F2 — Medium: stable capture IDs bound unstable payloads

The capture ID omitted the mind, but the stored payload included the mind,
retry-time wall-clock date, duration, and advisory counters. Repeating a capture
could conflict instead of replaying; another mind could collide with its receipt.

Fixed with capture-policy v2. Both identity and initial payload derive from the
mind, retained source evidence, configured extractor, and policy. Dates derive
from retained evidence when known. Advisory counters remain diagnostics rather
than durable evidence metadata.

Evidence: `adversarial_capture_replay_ignores_advisory_statistics` and
`adversarial_capture_date_comes_from_evidence_and_minds_are_isolated` failed before
the fix. `adversarial_stable_capture_payload_replays_through_managed_storage`
verifies actual receipt replay and distinct mind-scoped writes.

Compatibility: no database/schema-format change. Existing v1 records/receipts are
retained; the first v2 capture may create a new policy-versioned episode for an
earlier source. Repeats within v2 are stable. This is documented in the operator guide.

### F3 — High: excerpts could fabricate continuity across missing content

The assistant excerpt builder skipped an unreadable or memory-oversized chunk,
then appended later chunks. The reproducer changed a prefix plus missing content
plus suffix into the different statement `You should publish secrets`.

Fixed by retaining only a contiguous readable prefix and terminating at the first
gap. The builder uses a UTF-8-safe byte budget and marks truncation explicitly.

Evidence: `adversarial_assistant_excerpt_never_joins_across_an_unreadable_chunk`
failed before the fix. It exercises a blob exceeding memory's per-blob read limit.

### F4 — Medium: contradictory evidence metadata passed validation

Formation validation accepted multiple events at one sequence, an event ID that
disagreed with the source frontier, control characters in source IDs, and completed
extraction with no evidence.

Fixed by requiring strictly increasing evidence sequences, frontier identity
agreement, valid source identifiers, and available nonempty evidence for completion.

Evidence: the three `adversarial_formation_rejects_*` cases failed before the fix.
`adversarial_legacy_invalid_model_diagnostic_preserves_valid_source` ensures these
restrictions do not make existing valid source records unreadable merely because
their old extraction outcome records an invalid configured model. Metadata consistency
validation does not claim cryptographic authentication of imported evidence.

### F5 — Medium: bad optional model configuration prevented source persistence

An oversized model spec was accepted by the host builder, then rejected inside the
formation envelope validator. The episode was never stored even though source
capture itself was valid.

Fixed by rejecting blank, control-containing, and oversized model configuration
at the extractor boundary, disabling only extraction with a content-free diagnostic.
The source can still persist. Capability configuration also clears stale embedding
handles when supplied availability is None.

Evidence: `adversarial_invalid_model_does_not_discard_source_evidence` produced zero
episodes before the fix and verifies source retention afterward.

### F6 — Medium: episode search did not honor its text-query contract

SQLite episode search did not escape quotes and raised an unterminated-string
error. InMemoryBackend missed quoted terms and title-only matches, and treated an
empty query differently. This affected the task-aware context path as well as tools.

Fixed by quoting literal terms in SQLite and using title/narrative term matching
with deterministic ranking in memory. Empty queries return an empty set. Numeric
ranking equivalence between different lexical implementations is not claimed.

Evidence: `adversarial_episode_search_sqlite` and
`adversarial_episode_search_inmemory` both failed before the fix.

## Ruled-out case and retained scope boundaries

A suspected loss of empty tool-result content was tested against canonical session
admission. The authority owner rejects empty final tool-result content before
capture. That fixture failure was not counted as a Wave 3 defect; its temporary
change was reverted.

Automatic rescheduling of pending extraction, broader context-cache convergence,
temporal applicability, and embedding-space/graph semantics remain their explicit
later-wave work. They are not represented as completed by this review.

## Verification evidence

Observed red: three core metadata cases, seven host cases, and both episode-search
backend cases. A missing test-fixture telemetry field was a compile error and is
not counted as behavioral red.

Final green at `987272da`:

| Check | Result |
|---|---|
| `just test-crate omegon-memory` | Passed |
| `cargo test -p omegon-memory --all-features --locked` | Passed: 93 unit tests, 12 formation tests, 9 initial-wave tests |
| `cargo test -p omegon-memory --no-default-features --locked` | Passed: 89 unit tests, 12 formation tests, 4 initial-wave tests |
| `cargo test -p omegon --bin omegon adversarial --locked` | Passed: eight host adversarial tests |
| `RUST_TEST_THREADS=1 just test-commit` after final compatibility refinement | Passed: 5,293 main-crate unit tests, default integration suites, and memory tests |
| `just clippy-changed` after final compatibility refinement | Passed for both affected crates and all targets, including formatting |
| Participating OpenSpec validation and staged whitespace checks | Passed |

Existing ignored/opt-in tests remain excluded. G0–G4 passed for this hardening
slice. The review and its recheck were performed by the same executor. No live-model
quality or cost claim is inferred from these deterministic tests.

Local transcripts under
`~/.local/share/opencode/shell/9153e6c974f56bff7f08cdf3cb1bb0749fa67fbe/`:

- `sh_081ff28c2001hzvTscexxuEkCv.out`: metadata red.
- `sh_08200f8ce00185i1TgL3yzHmKO.out`: seven host behavioral failures.
- `sh_0820d45d1001yJpsAMSi8DFNGD.out`: episode-search red on both backends.
- `sh_08210ceec001zet5PQuwTJBNzl.out`: initial focused core green.
- `sh_08210d080001D0PmD5OGFdcsDQ.out`: eight host adversarial cases green.
- `sh_08216290d001yGC984pFFKk4XM.out`: feature variants and serial affected-crate gate.
- `sh_0821fa0510011dVwMa3R3syZfd.out`: final compatibility/feature/host recheck.
- `sh_0821fa468001Fky4Di1BWXQ3YN.out`: final Clippy recheck.
- `sh_082230639001AHKV8yjVM9BiXy.out`: final serial affected-crate gate on the corrected compatibility behavior.
