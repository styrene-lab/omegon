# Provenance and applicability design

## Domain ownership

`omegon-memory/src/types.rs` owns persisted vocabulary. Add additive evidence and
applicability records, with explicit schema migration where SQLite requires it.
The host supplies stable session, workspace, artifact, and observed-outcome
identities. It must not translate an assistant assertion into verified evidence.

Provenance records source kind, authority, evidence references, extraction identity
where applicable, observed time, and verification evidence. Applicability can
describe workspace, platform, component, and revision constraints. Valid time
describes when a claim applies; recorded time describes when memory learned it.
Do not conflate either with the Lamport mutation version.

Resolve the canonical revision descriptor and evidence retention policy before
freezing wire fields. Reuse existing host evidence identities rather than creating
a second session log. References whose source is missing become unavailable, not
fabricated evidence and not deletion instructions.

## Admission and mutation

Preserve `baseline/memory/lifecycle.md`: explicit structured conclusions can be
admitted automatically, inferred lifecycle summaries require confirmation, and
proposal-stage chatter is not durable truth. Persist authority at the boundary.
Validate that the claimed source reference belongs to the declared scope. Apply
supersession, provenance, edges, and mutation receipts atomically with version
preconditions. A retry returns its receipt rather than creating a new candidate.

## Persistence and visibility

Round-trip provenance and validity through DB, JSONL, and applicable vault formats.
Legacy fields map to legacy/unknown evidence, not confirmed observation. Preserve
existing operational metadata when imports update content metadata. Explicitly
document portable fields versus local access/salience metadata.

Render compact provenance with an evidence handle. Treat imported content as data;
its text cannot change authority, applicability, or harness instruction precedence.
An evidence reference provides attribution, not authorization to execute content.
