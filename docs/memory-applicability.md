# Memory applicability

Applicability describes where and when a fact is intended to apply. It is separate
from lifecycle status, confidence, provenance, and source availability.

## Record scope

`memory_store` accepts optional `applicability` rules. For example:

```json
{
  "section": "Constraints",
  "content": "Use the Linux-specific file-watcher workaround.",
  "applicability": { "platforms": ["linux"] }
}
```

Rules support these fields:

| Field | Meaning |
|---|---|
| `platforms` | Lowercase runtime OS identifiers, such as `linux` or `macos` |
| `workspaces` | Exact runtime workspace IDs |
| `revisions` | Exact `git:<full lowercase commit OID>` values |
| `components` | Caller-defined, case-sensitive component labels |
| `valid_from` | Inclusive RFC3339 validity boundary |
| `valid_until` | Exclusive RFC3339 validity boundary |

Values within a list are alternatives. Different dimensions combine with AND.
An empty list imposes no rule for that dimension. Empty rules or missing target
information produce `unknown`, rather than a claim of universal applicability.
`matches` means the recorded rules match; it does not prove unrecorded conditions.

Each list is limited to 16 identifiers. Individual identifiers and their aggregate
size are bounded. Revisions require full commit IDs; branch names and ancestry
ranges are not revision identities in this contract.

## Update an existing fact

Read its current version with `memory_inspect`, then call
`memory_set_applicability` with `fact_id`, `expected_version`, and `applicability`.
For example, replacing the sample ID and version with the inspected values:

```json
{
  "fact_id": "example-fact",
  "expected_version": 7,
  "applicability": { "valid_until": "2026-09-01T00:00:00Z" }
}
```

The update records when the rules were assigned and advances the fact version.
It does not reinforce, confirm, archive, or reactivate the fact. A stale version
rejects the update. An empty object explicitly replaces existing rules with unknown
applicability. Scope belongs to the fact record and does not automatically transfer
to a distinct replacement claim.

## Query context

Hosted current retrieval uses the runtime OS, canonical checkout workspace ID,
current Git HEAD, and UTC evaluation time. Workspace IDs use the existing runtime's
path-based identity; moving a checkout can change the ID. Git HEAD is a base commit
identity, not an assertion that the working tree is clean. Unavailable identities
remain unknown, and components require explicit caller information.

`memory_recall` and `memory_search_archive` accept a `context` object with
`platform`, `workspace`, `revision`, `component`, and `at`. Supplying it replaces
implicit host context. Omitted dimensions are unknown; current queries default
missing time to now. `memory_inspect` reports the host context it evaluated.

For a historical valid-time query:

```json
{
  "query": "file-watcher workaround",
  "context": { "at": "2026-08-31T12:00:00Z" }
}
```

The chosen tool still determines the lifecycle population: recall searches active
records, while archive search searches archived, dormant, and superseded records.
Historical queries without explicit context do not apply today's host scope.
Valid time is distinct from recorded time: a rule learned later can describe an
earlier period. This does not reconstruct the database's knowledge at an old date.

Known mismatches are excluded before lexical, vector, and graph candidate limits.
Unknown applicability is disclosed. Ambient and `request_context` memory selection
use the same rules. Existing injections are retired when eligibility changes or
cannot be revalidated; stored facts are preserved. `memory_query` is explicitly a
stored-state inventory, not applicability-filtered guidance.

Standalone embedders can supply target context with
`MemoryProvider::with_applicability_context` or `SearchFilter.context`. Without a
workspace binding, workspace and revision remain unknown. Scoped tool operations
use an operation namespace local to the provider instance; managed runtime operations
retain their session-bound identities.

## Persistence and compatibility

Schema v13 adds nullable applicability metadata. The existing migration workflow
supports v5–v12 stores and preserves legacy unknowns. Scope writes and receipts are
atomic. Legacy JSONL updates that omit scope preserve existing rules.

Scoped facts export as `applicable_fact`, rather than ordinary `fact`, so an older
reader cannot silently interpret scoped data as unrestricted knowledge. Use a
scope-aware binary for these exports. JSONL is the lossless record transport;
vault section pages display declared rules and their recorded time as a readable
projection. Neither transport nor publication reinforces facts.

Eligibility scanning retains bounded candidates but can inspect more rows when many
matches are inapplicable. This change makes no large-corpus latency or model-quality
claim.
