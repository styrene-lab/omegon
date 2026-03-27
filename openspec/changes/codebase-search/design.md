# codebase_search — AST-aware code retrieval with memory seeding — Design

## Architecture Decisions

### Decision: Two separate indexes (code_chunks + knowledge_chunks) in one SQLite cache, unified tool with scope parameter

**Status:** decided
**Rationale:** Code chunks need AST-based chunking (tree-sitter); knowledge chunks need heading/section-based chunking (markdown parsing, JSON flattening). Different chunking strategies, same BM25 ranking and SQLite cache. Keeping them in separate tables prevents cross-contamination of result scoring while allowing a unified search interface. The scope parameter (code/knowledge/all) lets callers target the layer they need.

### Decision: Codescan cache is separate from session memory — stored in .omegon/codescan.db, keyed by (path, content_hash)

**Status:** decided
**Rationale:** Code-structure and knowledge-structure facts have fundamentally different invalidation semantics from session facts: they go stale when files change, not when time passes. Mixing them into the main memory tier (facts.db) would corrupt the Ebbinghaus decay model and bloat the session context injection. A separate codescan.db with (path, content_hash, indexed_at) as the cache key allows per-file invalidation: only files whose content_hash has changed since last index need re-chunking. The main memory system remains focused on semantic/architectural facts curated by the agent; codescan is a mechanical index of raw structure.

### Decision: Lazy indexing on first query; background incremental reindex when git HEAD changes

**Status:** decided
**Rationale:** Eager indexing at session startup adds latency for projects where codebase_search is never called. Lazy on first query is the right default. Background incremental reindex (checking git HEAD at query time, spawning a tokio::spawn task to rechunk changed files) gives freshness without blocking the agent turn. The incremental nature (per-file content_hash comparison) makes reindex fast — typically only a handful of files change between sessions.

### Decision: Retrieval returns raw line-anchored chunks; no summarization pass; agent uses read tool for depth

**Status:** decided
**Rationale:** Summarizing chunks before returning them loses precision and adds latency/tokens. The agent's existing read tool (with offset/limit) is already the right mechanism for going deeper into a discovered chunk. codebase_search returns (file, start_line, end_line, chunk_type, score, content_preview) where content_preview is the first 300 chars. The agent then decides whether to read the full chunk. This keeps the retrieval tool cheap and composable.

## Research Context

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

LSP answers precise navigation questions about *known* symbols. `codebase_search` answers
discovery questions about *unknown* relevance. The agent needs both: LSP to navigate once it
knows what it's looking for, `codebase_search` to build the right context window before it
knows which symbols matter.

Shared dependency: tree-sitter. LSP client implementation and `codebase_search` both need
AST parsing. The tree-sitter crates (`tree-sitter`, `tree-sitter-rust`, `tree-sitter-python`,
etc.) should be factored into a shared `omegon-codescan` crate rather than duplicated.

### Memory Seeding Integration

The indexing pass produces a complete structural map: modules, types, functions, their
relationships and locations. This is exactly the architectural knowledge that currently has to
be manually discovered each session via bash + `memory_store` calls.

Three integration modes with the memory system:

**1. Index-time seeding**
On first index (or after detected git HEAD change), write structural facts directly to project
memory. Example outputs:
- `Architecture: "styrene-lxmf depends on styrene-rns for transport; LXMF router owns delivery"`
- `Architecture: "Identity key material in styrene-identity/src/identity.rs lines 44–112"`

The agent arrives at a new project already knowing its structure rather than rediscovering it.

**2. Retrieval-time routing**
Memory facts (architectural decisions, known file locations) can pre-filter and weight BM25
search. Semantic memory as a retrieval hint layer on top of syntactic search.

**3. Mind/persona seeding**
Personas with minds (memory stores) can be instantiated with codebase-indexed knowledge.
A "Rust Developer" persona in styrene-rs would arrive knowing the module structure, key types,
and dependency graph — genuine project-specific knowledge, not generic expertise.

### Invalidation strategy

Code-structure facts have different staleness semantics than session facts:
- Session facts: decay by time (Ebbinghaus model, already implemented)
- Code-structure facts: invalidated by file changes, not time

Proposed: code-structure facts stored with a `source_hash` field (file content hash or git
tree SHA). On session start, `codebase_search` checks if indexed facts are stale against
current git HEAD. Changed files invalidate only their associated facts — not the full index.

This is a cache-with-invalidation, not a decay system. The memory schema needs a `source_hash`
field and an invalidation query to support this cleanly.

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

**Index B: KnowledgeIndex** — structural markdown + JSON parsing
- Input: `docs/*.md`, `openspec/**/*.md`, `openspec/**/*.json`, `.omegon/*.json`, `ai/memory/facts.jsonl` (if present)
- Chunking strategy:
  - Markdown: heading hierarchy → each `##` section is a chunk, with frontmatter as metadata
  - YAML frontmatter in design docs: id, title, status, tags → filterable fields
  - JSON/JSONL: each top-level object as a chunk
- Chunk = (file, section_heading, start_line, end_line, tags, text)
- BM25 over heading + body text; tags as exact-match filter

**Why this is not collision but composition:**
The knowledge index doesn't need tree-sitter at all. A markdown parser (`pulldown-cmark` already used in the Rust ecosystem) suffices for heading-based section extraction. These two indexes can coexist in the same SQLite cache (separate tables: `code_chunks` and `knowledge_chunks`) and the `codebase_search` tool unifies them with a `scope` parameter.

**Unified tool interface:**
```
codebase_search(
  query: str,
  scope: "code" | "knowledge" | "all",  // default "all"
  max_results: int,                       // default 10
  tags: [str]                             // optional filter for knowledge index
) -> Vec<SearchChunk>
```

The knowledge index is what makes `codebase_search` the architectural reasoning layer — searching `docs/` design decisions, `openspec/` specs, `.omegon/` profile/history gives the agent immediate access to "why was this done" context that today requires multi-step memory_recall + manual file reading.

**Scale of knowledge index:**
- `docs/*.md` — 263 files, ~27k lines total — design tree nodes
- `openspec/` — ~30-40 active change directories with proposal/spec/tasks markdown
- `ai/memory/facts.jsonl` — up to 2,586 facts (this session) — structural facts as chunks
- `.omegon/milestones.json` — structured release state
- `.omegon/agents/*.md` — agent profiles

Total knowledge corpus: ~500-600 chunks at typical design-doc density. BM25 over this is trivial (no embedding needed, pure token overlap scoring).

## File Changes

- `core/crates/omegon-codescan/` (new) — New crate — CodeScanner (tree-sitter AST over .rs/.ts/.py/.go), KnowledgeScanner (markdown/JSON/JSONL parsing), BM25Index, ScanCache (SQLite .omegon/codescan.db).
- `core/crates/omegon-codescan/src/code.rs` (new) — tree-sitter-based chunker for Rust, TypeScript, Python, Go. Emits code_chunks: (path, start_line, end_line, item_name, item_kind, text).
- `core/crates/omegon-codescan/src/knowledge.rs` (new) — Markdown heading-hierarchy chunker + JSON/JSONL flattener. Emits knowledge_chunks with frontmatter metadata extraction (id, title, status, tags).
- `core/crates/omegon-codescan/src/bm25.rs` (new) — BM25 ranking implementation over both chunk tables. Shared by code and knowledge searches.
- `core/crates/omegon-codescan/src/cache.rs` (new) — SQLite-backed chunk cache at .omegon/codescan.db. Keyed by (path, content_hash). Incremental invalidation.
- `core/crates/omegon/src/tools/codebase_search.rs` (new) — codebase_search and codebase_index agent tools backed by omegon-codescan. Registers with tool_registry.
- `core/crates/omegon/src/tool_registry.rs` (modified) — Register CODEBASE_SEARCH and CODEBASE_INDEX tool names.
- `core/Cargo.toml` (modified) — Add omegon-codescan to workspace members.

## Constraints

- tree-sitter grammars must be embedded (linked at compile time, no runtime download) — use tree-sitter-rust, tree-sitter-typescript, tree-sitter-python, tree-sitter-go crates.
- Knowledge indexer covers docs/, openspec/, .omegon/ (json/md), ai/memory/facts.jsonl — NOT ai/memory/facts.db (use JSONL export instead of querying SQLite directly).
- BM25 must run in-process with no external search process dependency (Tantivy or manual implementation).
- codescan.db must not be committed — add to .gitignore.
- Indexing a 263-file docs/ corpus must complete in under 2 seconds on a modern laptop.
- Result chunks must include enough context for the agent to judge relevance without a follow-up read call (300-char preview minimum, 1000-char maximum).
