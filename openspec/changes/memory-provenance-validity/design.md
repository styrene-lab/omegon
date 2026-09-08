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

## Wave 5 transport prerequisite

Export all fact statuses before adding richer validity metadata. Active-only exports
lose correction history and can leave exported edges without their historical endpoints.
The additive `JsonlFact.operational` object carries persisted confidence, reinforcement
count/time, decay rate, last access, source session, lifecycle timestamps, and jj change identity.
Computed relevance and effective decay scores remain transient.

This changes the earlier stable-diff choice to omit operational metadata: reliable
round-trip state takes priority over suppressing legitimate persisted-state changes.
Newer modern records replace operational state; legacy records preserve it on update.
New legacy imports keep the existing initialization defaults. Equal/older versions
remain no-ops, including metadata-only differences. An absent source uses an empty
SQLite source value mapped to `None`, rather than inventing the label `manual`.
No schema change is needed for this transport slice; the database stays at v10.

Validate operational numeric bounds and RFC3339 timestamps before mutation. Reject
semantic validation failures atomically with the import batch and its receipt.
Existing malformed-line accounting remains in place. Export must propagate fact-row
read errors instead of returning an apparently complete document with missing history.
