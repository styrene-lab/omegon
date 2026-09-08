# Evidence capture design

Depend on provenance and independent extraction readiness. The host captures
committed user-visible messages, tool outcomes, lifecycle conclusions, artifact
references, and finalization state. `loop_session.rs` must expose an evidence
reference/checkpoint rather than only the current 200/300-character fragments.
The session log remains the source of truth. Memory owns structured episodes and
candidate formation contracts.

An episode includes goal, applicable workspace/revision, decisions and rationale,
attempts with observed outcomes, corrections, changed artifacts, verification,
unresolved work, and source references. Keep administrative counts as metadata.
Absence of a tool success must remain unknown verification, not a successful result.

Use evidence watermarks and stable identities `(session, checkpoint, extraction
policy version)` for durable work. Commit the checkpoint reference before scheduling
extraction. Track pending/running/completed/failed work with bounded retries and
recovery after interruption. Content plus operation identity binds a retry. Changed
input or changed extraction policy gets an explicit new work version. Persist a
selected extraction result before replaying its admission mutations so stochastic
retries cannot alter an already committed batch.

Checkpoint on bounded committed-event accumulation and before evidence eviction;
session end flushes the last watermark. Use managed lifecycle resources and
cancellation. Queue saturation retains recoverable source watermarks and reports
backpressure. Do not block the interactive loop on unbounded extraction.

Require structured extractor output: candidate content, section/kind, evidence
references, applicability, and inferred/observed classification. Reject unknown
source handles and malformed candidates independently of valid siblings. Admission
occurs through the later reconciliation contract. Until then retain typed candidates
without representing them as verified durable knowledge.
