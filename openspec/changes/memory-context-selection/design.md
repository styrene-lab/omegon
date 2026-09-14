# Context selection design

Depend on retrieval intent/filtering and provenance/applicability. The host supplies
task signals, workspace applicability, pins, a memory-specific budget, and token
accounting. `omegon-memory` owns selection and returns selected evidence plus
explanations. `MarkdownRenderer` only formats that selection.

Use a small core/pin allocation followed by task-relevant facts, episodes, and
procedures. Deduplicate pins and recalled facts by identity. Pinning changes
selection priority, not validity or truth confidence. An archived, superseded,
or known-inapplicable pin cannot silently become established current guidance.
Where a replacement exists, return an explicit resolution result.

Evaluate candidates before grouping presentation sections. Skip an oversized
candidate and continue considering smaller candidates. Account for headings,
provenance, and separators. Use the selected model's token counter when available;
otherwise a tested conservative fallback with final host budget enforcement.
Four characters per token is not a universal safe bound.

All surfaces use the same eligibility and packing owner, while explicit queries
may choose a different declared selection intent from ambient orientation.
No-query low-signal turns get only justified core/pinned memory. Resolve the
routine cap and per-kind allocations using the evaluation development corpus.

Cache selected snapshots by task/applicability fingerprint, memory version,
pin identity, policy version, token budget, and time-sensitive eligibility expiry.
TTL is not permission to reuse stale scope or archived facts. Use invalidation to
avoid full store scans and runtime creation on every unchanged turn.

Selection reports carry IDs, counts, rejection reasons, degradation, and budget
accounting through existing semantic status. Store content only in appropriate
debug/test artifacts, never routine info-level logs. Compare no-memory/file-search
baselines and retain prompt-cache stability as an evaluation metric.

## Wave 5 counted selection

Use a shared domain selector and managed `SelectContext` request for ambient memory
and explicit memory packs. `MemoryProvider` uses the same selector with its renderer
hook. Selection owns eligibility, pin priority, deduplication, and whole-block budget
decisions; formatting is delegated to the renderer and counted before acceptance.

The default memory cap is 1,024 accounted tokens, intersected with the host allocation.
`memoryContextTokens` configures the hosted cap at startup; zero disables injection,
and the maximum is 8,192. Pins have first claim on the bounded allocation, followed
by task evidence. Episodes use at most one quarter of the allocation, counted as
their formatted section, and require a nontrivial query. Empty/control-only ambient
turns use eligible pins rather than an inventory dump. Explicit queries declare a
different intent and can request otherwise low-signal terms.

The host currently has no model-specific exact tokenizer in this path. The fallback
counts each UTF-8 byte as an accounted unit and identifies that method in reports.
It is a conservative policy, not an exact measurement or a universal tokenizer bound.
Callers can provide a deterministic exact counter for their tokenizer. The selector
counts complete emitted text, including headings, separators, versions, and scope
qualifiers. Outer provider-message framing remains the host's responsibility.

Candidate processing and report detail are bounded. Selection reports describe the
inputs examined by the selector; they do not enumerate every row rejected by an
upstream retrieval channel. Superseded pins resolve through the backend's existing
lineage lookup, then undergo the same eligibility checks as other candidates.
`memory_selection` exposes the last ambient report; explicit packs carry their own
reports, including empty selections. Reports contain handles and reasons, not fact
contents. Semantic caching remains a separate follow-up; TTL alone never authorizes
reuse of stale eligibility.
