# Memory context selection

The memory selector packs complete claims into a memory-specific allocation. Hosted
ambient memory, standalone `MemoryProvider`, and `request_context` memory packs use
the same eligibility and packing owner.

## Budget and accounting

Set `memoryContextTokens` in the project profile to configure the hosted cap at
startup. The default is 1,024 accounted units, intersected with the host allocation.
Zero disables memory injection. The maximum is 8,192; direct JSON values above that
are clamped. Standalone callers can use `with_memory_token_cap`.

The hosted path currently counts UTF-8 bytes and reports
`conservative_utf8_bytes`. This is a conservative accounting policy, not an exact
measurement or a universal bound for every provider tokenizer and normalization
scheme. Embedders can supply a deterministic exact `MemoryTokenCounter` and identify
its tokenizer. Provider-message framing and final prompt enforcement remain with
the host.

The selector counts the complete formatted memory block, including headings,
separators, fact versions, and applicability qualifiers. A candidate that does not
fit is skipped; later smaller candidates are still considered. Claims are not
truncated to force them into the allocation.

Development fixtures containing twelve representative constraints produced:

| Cap | Selected facts | Accounted UTF-8 units |
|---|---:|---:|
| 512 | 3 | 480 |
| 1,024 | 6 | 900 |
| 2,048 | 12 | 1,745 |

The middle cap bounds routine volume while retaining several complete constraints.
These are deterministic packing fixtures, not model-quality or latency benchmarks.

## Eligibility and priority

Pins are considered first, followed by task-retrieved facts. Lifecycle status,
applicability, and the confidence floor still apply. Duplicate identities consume
space once. A superseded pin can resolve to its active replacement; the report
records the mapping and the replacement undergoes the same eligibility checks.

Empty and control-only ambient turns use eligible pins instead of an inventory
dump. Explicit context requests declare a separate intent and can query short terms.
Episodes require a nontrivial query and consume at most one quarter of the memory
allocation, including their section formatting. No unrequested global expansion is
introduced by selection.

Candidate processing, episode inputs, and exclusion detail are bounded. Reports
cover inputs examined by the selector, not every record filtered by upstream
retrieval. Storage is not reinforced or reactivated by selection.

## Inspect decisions

`memory_selection` returns the last ambient selection report. It includes:

- selected IDs, versions, and evidence kinds;
- exclusion counts and bounded details for budget, lifecycle, applicability,
  confidence, duplication, input limits, and low signal;
- pin replacement mappings;
- accounting method, effective budget, accounted usage, and budget exhaustion;
- retrieval degradation when a selection could not be produced.

Explicit `request_context` memory packs carry their own report in the pack's
`selection` details, including empty selections. Report payloads contain handles
and decisions rather than fact contents.

Static providers and runtime features both replace injections by source, including
finite-TTL and empty replacements. An old memory block cannot reappear after its
clearing replacement expires.

## Semantic cache

Each owner retains one selection snapshot. Reuse requires the same task, mind, pin
order, applicability target, intent, candidate limit, budgets, tokenizer accounting,
renderer identity, and policy version. Backend-instance identity prevents reuse
across reopened or replaced stores. Local writes and external SQLite commits
invalidate the snapshot.

Snapshots retain their ranking for at most 30 seconds. They expire sooner at the
next applicable valid-time boundary or confidence-floor transition. Timing checks
include currently excluded future facts, so a claim becoming eligible can force
retrieval even though it was absent from the old selection. Backward clock or
query-time movement also forces a miss.

Cache hits check the backend stamp and deadlines instead of repeating retrieval
and scanning fact contents. Timing metadata is scanned on misses. A concurrent
write during computation prevents that result from becoming a reusable entry.
Zero-budget and empty control-only requests avoid this cache metadata work.

`memory_selection` includes `cache_hit`, `selected_at`, and `cache_expires_at`.
Expiry is a wall-clock ceiling, not a promise of validity despite changed inputs
or storage. Ranking can reflect the earlier selection time within that bounded
window; continuously recomputed fractional decay ranking is not claimed.

Custom backends opt in with reliable selection revision and timing contracts.
Custom renderers opt in with a stable `memory_cache_identity`; the default is
uncached. Changing a renderer or tokenizer's behavior requires a different identity.
