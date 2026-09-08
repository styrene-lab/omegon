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
