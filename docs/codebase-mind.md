+++
title = "Codebase Mind"
tags = ["design","codescan","memory","codebase-mind"]
+++

# Codebase Mind

---
title: Codebase Mind
status: exploring
tags: [design, codescan, memory, codebase-mind]
---

# Codebase Mind

## Overview

Codebase Mind is Omegon's durable structural memory for repositories. It grows out of `omegon-codescan`, but its target is larger than BM25 chunk search: it should know files, symbols, relations, manifests, chunks, freshness, confidence, and scopes well enough that agents can query repo structure without repeatedly rereading the same files.

The core design principle is **structural truth with provenance**. Every fact must carry where it came from, how it was extracted, how fresh it is, and how confident the harness should be.

## Position in the harness

Codebase Mind complements the existing memory/lifecycle surfaces:

- Memory Mind: durable semantic facts, patterns, decisions.
- Lifecycle Mind: design nodes, OpenSpec changes, tasks, readiness.
- Codebase Mind: repo files, symbols, relations, chunks, manifests, freshness.

The existing `codebase_search` and `codebase_index` tools become query/index views over Codebase Mind. Future tools can expose graph neighborhoods and affected-symbol queries without inventing a second store.

## Seed implementation

Use `omegon-codescan` as the seed crate. Do not create a new crate until the internal schema proves stable.

Current primitive componentry already exists:

- file discovery and skip policy in `indexer.rs`
- language scanners under `code/languages/`
- chunk cache in SQLite
- BM25 search over code/knowledge chunks
- `codebase_search` / `codebase_index` tool adapter in `omegon`
- affected-crate validation tooling for fast local feedback

## Target pipeline

```text
discover → extract → persist → invalidate → query
```

Stages:

1. Discover files under a scoped root using a security/ignore policy.
2. Extract chunks now; symbols and relations next.
3. Persist content hashes, chunks, symbols, relations, and index metadata.
4. Invalidate by content hash, git HEAD, dirty working tree state, and scope.
5. Query by BM25, graph neighborhood, affected paths/symbols, and support/freshness diagnostics.

## Data model direction

### CodeChunk

Existing BM25/search unit. Add extraction metadata:

- `language`
- `strategy`: `tree_sitter | regex | manifest | knowledge`
- `confidence`: `extracted | inferred | ambiguous`

### CodeSymbol

Future graph node:

- stable id
- path
- name
- kind
- line range
- parent id/name
- language
- extraction strategy
- confidence
- content hash / observed revision

### CodeRelation

Future graph edge:

- source id
- target id
- relation: `contains | imports | calls | implements | extends | depends_on | documents | tests | mentions`
- source file/location
- strategy
- confidence

### FreshnessReport

Tool-facing diagnostic:

- indexed root/scope
- indexed git HEAD
- current git HEAD
- dirty relevant paths
- untracked relevant paths
- stale/unknown status
- last indexed timestamp
- files scanned/skipped

## Implementation phases

### Phase 1 — metadata and freshness foundation

- Add extraction strategy/confidence/language metadata to `CodeChunk`.
- Store metadata in `code_chunks`.
- Preserve backward-compatible cache behavior where possible.
- Fix HEAD-only fast path so dirty relevant working-tree changes prevent stale fast-path reuse.
- Surface freshness in `codebase_index` / `codebase_search` details.

### Phase 2 — graph-ready extraction schema

- Add `CodeSymbol`, `CodeRelation`, and `CodeExtraction` structs.
- Let scanners return `CodeExtraction` internally, then derive `CodeChunk` from symbols/text ranges.
- Keep BM25 search unchanged externally.
- Start with `contains` relations and parent/child symbol relationships.

### Phase 3 — persistence expansion

- Add SQLite tables for symbols, relations, files/index metadata.
- Version the schema.
- Add scoped-pruning rules so partial indexes do not delete facts outside their scope.
- Keep SQLite facts databases as ignored operational caches; they are rebuilt from source plus optional projections and are not mergeable git artifacts.
- Add deterministic JSONL projection export/import for reviewable, git-friendly structural snapshots.

### Phase 4 — query tools

Add tool views over the same store:

- `codebase_graph`
- `codebase_neighbors`
- `codebase_affected`
- `codebase_freshness`

### Phase 5 — lifecycle/memory integration

- OpenSpec/design file scopes can query Codebase Mind.
- Cleave planning can use affected-symbol/file neighborhoods.
- Memory can store high-level architectural facts derived from graph evidence, not raw graph facts.

## Persistence and git policy

Codebase Mind uses a two-tier persistence model.

### Operational facts database

SQLite facts databases are local operational caches and are ignored by git:

```text
.omegon/codescan.db
.omegon/codebase-mind/facts.db
```

Reasons:

- SQLite databases are binary and noisy in diffs.
- They are mutated by local indexing and concurrent agents.
- They can include machine-local freshness state and transient dirty-worktree observations.
- They are rebuildable from source files plus optional projections.

### Git-friendly JSONL projections

Reviewable structural snapshots should be exported as deterministic JSONL under:

```text
ai/codebase/
  snapshot.json
  files.jsonl
  symbols.jsonl
  relations.jsonl
  manifests.jsonl
```

Projection rows must be stable and provenance-rich. Symbols sort by `path, kind, name, start_line, id`; relations sort by `source, relation, target, source_file, source_location`.

Do not include local absolute paths, machine ids, volatile timestamps, WAL/cache internals, or dirty working-tree observations in committed projections.

A projection snapshot records canonical repo state, for example:

```json
{"schema":1,"kind":"snapshot","git_head":"<sha>","root":".","generated_by":"omegon-codescan"}
```

Import/export tools should treat projections as a seed/review artifact, not as the live mutable store. `facts.db` remains the runtime index; JSONL is the durable git projection.

## Design constraints

- Every structural fact needs provenance.
- Regex-backed language support must be marked lower-confidence than tree-sitter support.
- Dirty working-tree files must never be silently ignored by freshness diagnostics.
- Scoped indexing must not prune facts outside the scoped root.
- Tool adapters should remain thin; extraction and freshness belong in `omegon-codescan`.
- Avoid LLM-derived structural facts until deterministic extraction and freshness are solid.

## Open questions

- [assumption] `omegon-codescan` can remain the seed crate until symbol/relation persistence stabilizes.
- What is the stable symbol id format across file moves and renames?
- Should manifests (`Cargo.toml`, `.sln`, `pom.xml`, `package.json`) be first-class code facts or a separate manifest fact class?
- How much graph querying belongs in SQLite vs in-memory indexes?
- Should Codebase Mind publish durable memory facts automatically, or only when an agent/operator promotes a finding?
- Which projection files should be generated by default once symbols/relations exist: all of `files`, `symbols`, `relations`, `manifests`, or only changed scopes?

## Immediate next slice

Implement Phase 1:

1. Add `ExtractionStrategy` and `ExtractionConfidence`.
2. Add `language`, `strategy`, and `confidence` to `CodeChunk`.
3. Update language modules to pass metadata.
4. Update SQLite cache schema and round-trip tests.
5. Add dirty-working-tree guard for the HEAD fast path.
6. Keep validation scoped to `omegon-codescan`.
