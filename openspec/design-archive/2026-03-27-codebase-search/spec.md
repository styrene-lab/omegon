# codebase_search — AST-aware code retrieval with memory seeding — Design Spec (extracted)

> Auto-extracted from docs/codebase-search.md at decide-time.

## Decisions

### Two separate indexes (code_chunks + knowledge_chunks) in one SQLite cache, unified tool with scope parameter (decided)

Code chunks need AST-based chunking (tree-sitter); knowledge chunks need heading/section-based chunking (markdown parsing, JSON flattening). Different chunking strategies, same BM25 ranking and SQLite cache. Keeping them in separate tables prevents cross-contamination of result scoring while allowing a unified search interface. The scope parameter (code/knowledge/all) lets callers target the layer they need.

### Codescan cache is separate from session memory — stored in .omegon/codescan.db, keyed by (path, content_hash) (decided)

Code-structure and knowledge-structure facts have fundamentally different invalidation semantics from session facts: they go stale when files change, not when time passes. Mixing them into the main memory tier (facts.db) would corrupt the Ebbinghaus decay model and bloat the session context injection. A separate codescan.db with (path, content_hash, indexed_at) as the cache key allows per-file invalidation: only files whose content_hash has changed since last index need re-chunking. The main memory system remains focused on semantic/architectural facts curated by the agent; codescan is a mechanical index of raw structure.

### Lazy indexing on first query; background incremental reindex when git HEAD changes (decided)

Eager indexing at session startup adds latency for projects where codebase_search is never called. Lazy on first query is the right default. Background incremental reindex (checking git HEAD at query time, spawning a tokio::spawn task to rechunk changed files) gives freshness without blocking the agent turn. The incremental nature (per-file content_hash comparison) makes reindex fast — typically only a handful of files change between sessions.

### Retrieval returns raw line-anchored chunks; no summarization pass; agent uses read tool for depth (decided)

Summarizing chunks before returning them loses precision and adds latency/tokens. The agent's existing read tool (with offset/limit) is already the right mechanism for going deeper into a discovered chunk. codebase_search returns (file, start_line, end_line, chunk_type, score, content_preview) where content_preview is the first 300 chars. The agent then decides whether to read the full chunk. This keeps the retrieval tool cheap and composable.

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

### Two-index architecture — code scanning + knowledge scanning

The user's insight: `ai/`, `.omegon/`, and `docs/` should be searchable alongside code, but they are fundamentally different document types with different chunking needs. This reveals a two-index architecture:

**Index A: CodeIndex** — tree-sitter AST over source files
- Input: `.rs`, `.ts`, `.py`, `.go` etc.
- Chunking boundary: function, struct/class, module-level item
- Chunk = (file, start_line, end_line, item_name, item_kind, text)
- BM25 over item names + body text

**Index B: KnowledgeInd…
