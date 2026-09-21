# Reconciliation design

Depend on structured provenance and captured candidates. Own candidate matching,
decision validation, and mutation planning in `omegon-memory`; inject semantic
classification through a host-provided interface rather than importing provider
clients into the domain crate.

Normalized hashes remain a fast path. Candidate lookup combines bounded lexical
and optional semantic matching filtered by applicability. A classifier may propose
equivalent, refinement, correction, or conflict relationships but cannot bypass
scope, evidence, authority, or version validation. Keep classifier policy identity
and selected decision as replayable evidence. On provider unavailability, retain
unresolved candidates; do not silently auto-merge based on text similarity alone.

Explicit user corrections and structured authoritative updates may supersede an
applicable predecessor. Mere recency is insufficient. Cross-platform statements
can coexist without being contradictions. Preserve conflicting claims and their
sources when evidence does not resolve them. Selection receives conflict groups
with uncertainty rather than unrelated authoritative bullets.

Admission plans use operation receipts and fact preconditions. Commit facts,
supersession links, candidate state, and evidence edges together. Count independent
evidence identities separately from repeated observations of the same source.
An archived/superseded claim is not reactivated just because an old import or a
paraphrase appears again. Explicit restoration requires current supporting evidence
and a recorded lifecycle decision.

Preserve existing lifecycle confirmation requirements. Ordinary explicit memory
writes need not introduce a new operator confirmation flow; they still pass the
domain's validation and reconciliation policy.
