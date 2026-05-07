+++
id = "47774561-f997-4bcc-a1d4-d270bade54ad"
kind = "document"
title = "codebase_search — AST-aware code retrieval with memory seeding"
status = "implemented"
tags = ["architecture", "tools", "code-intelligence", "memory", "lsp", "retrieval"]
aliases = ["codebase-search"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "feature"
open_questions = []
priority = "1"
related = ["lsp-integration"]
+++

# codebase_search — AST-aware code retrieval with memory seeding

## Overview

A `codebase_search(query, strategy)` tool backed by tree-sitter AST parsing and BM25 keyword
indexing. Answers concept-retrieval questions ("find code about packet fragmentation") that LSP
cannot answer and that the agent currently handles by guessing file paths and running grep.

Inspired by ATLAS's PageIndex component (itigges22/ATLAS), which replaced Qdrant vector RAG with
AST-aware chunking after finding that function/class boundaries are semantically meaningful chunk
boundaries while arbitrary token windows are not.

## Research

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

## Decisions

### Decision: Two-index SQLite cache (.omegon/codescan.db) with tree-sitter code scanner and markdown/JSON knowledge scanner

**Status:** decided
**Rationale:** Separate from facts.db (different invalidation: file content hash, not time decay). Code index uses tree-sitter AST for named declaration boundaries with regex fallback. Knowledge index uses pulldown-cmark heading-hierarchy for docs/, openspec/, .omegon/, and ai/memory/facts.jsonl. BM25 in-process. Lazy first-query indexing with HEAD-based fast-path (skip file walk when HEAD unchanged). Incremental reindex on HEAD change via rate-limited background tokio task (30s cooldown). Preview: 300 chars multi-line in table, 400 chars in JSON details.

## Open Questions

*No open questions.*

## Relations

- Builds on: `lsp-integration` (shared tree-sitter dependency, complementary layer)
- Feeds into: memory system (structural fact seeding, code-keyed invalidation)
- Feeds into: persona mind stores (project-specific knowledge at instantiation time)
- Inspired by: ATLAS PageIndex (itigges22/ATLAS — AST tree + BM25 hybrid retrieval)
