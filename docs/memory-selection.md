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
clearing replacement expires. Semantic selection caching is a subsequent Wave 5
step; current selection still revalidates the underlying data.
