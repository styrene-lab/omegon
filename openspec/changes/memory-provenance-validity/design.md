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

## Wave 5 inferred lifecycle candidate slice

`StoreLifecycleInference` creates a fresh `Pending` fact with zero reinforcement
and zero established-confidence prior. The operation receipt provides idempotency.
Content deduplication against active facts must not reinforce those facts; candidate
reconciliation across separate operation identities remains later work.

Schema v11 adds nullable `facts.lifecycle_inference`. Its typed attribution preserves
the caller's source kind, artifact type/path/subreference, and proposed supersession.
These are declared references, not validated observations or execution evidence.
`Fact.created_at` supplies recorded time. No workspace/revision applicability fields
are frozen in this slice. The reference is stored as data and is not dereferenced.

Inferences cannot enter current/historical recall, pinned context, vector indexing,
or vault fact materialization. Export/import retains the pending record and rejects
promotion by changing status or stripping metadata from an existing candidate.
Candidate source corruption is an error. Versioned confirmation and validated
explicit artifact admission remain open; this slice changes the inferred route.

## Wave 5 explicit artifact admission slice

Validate explicit claims in the host's artifact adapter. Use existing opsx parsers
for decided design decisions, implementation constraints, and baseline/archived
specification requirements. Match case-sensitive statement text after whitespace
normalization. Reject proposal paths, open questions, unsupported source kinds,
and missing or mismatched references before writing memory.

Read a bounded immutable snapshot through the existing descriptor-relative filesystem
helpers. Artifact paths are repository-relative and cannot traverse symlinks or `..`.
The snapshot SHA256 identifies the bytes parsed; it is not a claim that the file
will remain unchanged after admission or that an execution outcome was observed.

`LifecycleConclusionSource` is a typed, version-prefixed JSON envelope in the existing
portable `Fact.source` string. This retains wire/schema compatibility at v11 while
distinguishing new artifact attribution from legacy free-form source labels. The
envelope records artifact kind/path/id/subreference and artifact/statement hashes.
Its presence is provenance, not permission to execute the referenced content.

The domain lowers `StoreLifecycleConclusion` into existing atomic store/supersede
mutations. A correction requires an explicit target version and the same mind.
Receipts bind the conclusion and source snapshot to the operation identity. Without
a correction target, exact content/source duplicates reuse the normal reinforcement path; differing evidence
remains separately attributed for later reconciliation. Revalidation on tool retry
requires the same artifact snapshot; missing/changed sources are explicit errors.

## Wave 5 operator confirmation

Use the existing per-request TUI/ACP permission channel rather than accepting
model-provided confirmation flags. `memory_confirm` reads a pending candidate and
raises a typed runtime request containing its exact digest and preconditions. Only
the loop's approval handler dispatches `memory_apply_confirmation`, registered as
an internal invocation. The handler preserves the requesting invocation scope.

Storage rechecks the snapshot and candidate/target versions inside the atomic
mutation. Confirmation retains inference attribution, adds operator-surface review
metadata, and initializes active confidence/reinforcement. Proposed supersession
is part of the reviewed request and the same transaction. It never becomes evidence
of a successful tool execution merely because an operator accepted the claim.

Schema v12 is a semantic compatibility gate: v11 requires every attributed inference
to be pending, while v12 permits active/historical records with confirmation. The
existing nullable metadata column carries the additive confirmation record. Legacy
pending records remain unconfirmed on migration. Import cannot promote an existing
pending record or rewrite retained review metadata. Portable attribution does not
constitute a cryptographic proof of local operator presence.

Inventory read failures propagate so corrupt confirmation data cannot be mistaken
for removed facts during vault publication. Approval is bounded to 120 seconds;
headless callers without a responding operator surface do not receive confirmation.

## Wave 5 read-only provenance inspection

Add a status-neutral, mind-scoped lookup and a shared `FactInspection` projection.
Inspection preserves the requested record instead of substituting an active
replacement. It reports lifecycle status, stored timestamps and reinforcement,
bounded Unicode excerpts, a full-content digest, and the recorded attribution basis.
Legacy labels do not become evidence of operator approval or successful execution.

The host can resolve known lifecycle artifact references through the existing
descriptor-relative reader. Report matching/changed snapshots, readable unverified
references, unavailable sources, and unsupported references separately. The
standalone provider reports `not_checked` when it has no filesystem binding.
Reading a declared reference never turns an inference into an explicit conclusion.
Availability does not change confidence, status, applicability, or reinforcement.
No schema migration is required for this read-only projection.

## Wave 5 applicability

Schema v13 adds nullable recorded applicability to facts. Rules describe platform,
workspace, revision, component, and a valid-time interval. Lists are alternatives
within one dimension; dimensions combine conjunctively. Start is inclusive and
end is exclusive. Empty rules and missing required target context yield unknown
applicability. Any known mismatch excludes the fact from current selection.
Matching means that recorded rules match, not that unrecorded conditions are proven.

The host uses the existing runtime workspace ID derived from the canonical checkout
path. It is path-bound: moving a checkout can change the ID. Revision is the exact
`git:<full lowercase commit OID>` for HEAD, read without Git/jj subprocesses. It is
not a clean-working-tree attestation, branch name, ancestry range, or jj change ID.
Unavailable descriptors remain unknown. Component labels are explicit caller data.

Current managed requests freeze runtime OS, workspace, HEAD, and UTC evaluation time
once per request. Explicit query context replaces those defaults; omitted dimensions
are unknown, except that current queries default missing time to now. Historical
queries retain the archive population and do not use current host constraints unless
requested. `context.at` filters valid time, not historical database knowledge versions.
Recorded time is preserved separately and can postdate the interval being queried.

`StoreApplicableFact` stores rules atomically with a new fact. `SetFactApplicability`
checks the fact version, records when the rule was assigned, and leaves confidence,
reinforcement, and lifecycle status unchanged. Exact rule identity participates in
deduplication. Legacy updates that omit scope preserve known rules; an empty rule
object explicitly replaces them with unknown applicability.

FTS scans until its eligible over-fetch budget is filled; excluded rows do not consume
candidate slots. Vector and graph channels apply the same domain rules. Retained
candidate limits remain bounded, although highly selective filters can scan more
rows. No large-corpus latency claim is made. Ambient and request-context selection
share these rules, and renderers disclose unknown applicability. A changed HEAD or
expired rule can clear an existing context injection without a fact mutation.

Scoped facts export with the `applicable_fact` JSONL tag, so older readers cannot
silently interpret them as unrestricted `fact` records. Schema-v13 readers accept
both tags, validate recorded rules, and preserve scope through reopen and transport.
Vault section pages include declared scope as readable metadata; JSONL is the
lossless record transport. Inventory remains a stored-state view, not current guidance.
