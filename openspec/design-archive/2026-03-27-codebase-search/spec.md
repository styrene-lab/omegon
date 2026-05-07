+++
id = "7f2179eb-8aa8-4e07-b7ab-38b91da45d5e"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# codebase_search — AST-aware code retrieval with memory seeding — Design Spec (extracted)

> Auto-extracted from docs/codebase-search.md at decide-time.

## Decisions

### Two-index SQLite cache (.omegon/codescan.db) with tree-sitter code scanner and markdown/JSON knowledge scanner (decided)

Separate from facts.db (different invalidation: file content hash, not time decay). Code index uses tree-sitter AST for named declaration boundaries with regex fallback. Knowledge index uses pulldown-cmark heading-hierarchy for docs/, openspec/, .omegon/, and ai/memory/facts.jsonl. BM25 in-process. Lazy first-query indexing with HEAD-based fast-path (skip file walk when HEAD unchanged). Incremental reindex on HEAD change via rate-limited background tokio task (30s cooldown). Preview: 300 chars multi-line in table, 400 chars in JSON details.

## Research Summary

### Relationship to LSP

These are complementary layers at different levels of the code-intelligence stack:

```
codebase_search          LSP
────────────────         ────────────────────────────
"find code about X"      "where is symbol Y defined"
discovery mode           navigation mode
no server required       requires language server
tree-sitter + BM25       full type system
works on any project     needs per-language setup
```

LSP answers precise navigation questions about *known* symbols. `codebase_search` answer…

### Memory Seeding Integration

The indexing pass produces a complete structural map: modules, types, functions, their
relationships and locations. This is exactly the architectural knowledge that currently has to
be manually discovered each session via bash + `memory_store` calls.

Three integration modes with the memory system:

**1. Index-time seeding**
On first index (or after detected git HEAD change), write structural facts directly to project
memory. Example outputs:
- `Architecture: "styrene-lxmf depends on styrene-rns…

### Invalidation strategy

Code-structure facts have different staleness semantics than session facts:
- Session facts: decay by time (Ebbinghaus model, already implemented)
- Code-structure facts: invalidated by file changes, not time

Proposed: code-structure facts stored with a `source_hash` field (file content hash or git
tree SHA). On session start, `codebase_search` checks if indexed facts are stale against
current git HEAD. Changed files invalidate only their associated facts — not the full index.

This is a cache-…

### Implementation sketch

```
omegon-codescan/        Shared crate
  src/
    ast/                tree-sitter parsing → ASTNode tree
    bm25/               BM25 index (rank-bm25 or manual)
    hybrid/             Combined AST tree search + BM25
    indexer/            Repo walker, builds index
    seeder/             Writes structural facts to memory

Tool: codebase_search(query: str, strategy: "ast" | "bm25" | "hybrid") -> Vec<CodeChunk>
Tool: codebase_index(invalidate: bool) -> IndexStats
```
