+++
id = "33a030e6-3a9f-484f-aa7a-79304850d6ae"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# codebase_search — AST-aware code retrieval with memory seeding

## Intent

Implement a `codebase_search` tool backed by a two-index architecture:
- **Code index**: tree-sitter AST chunking over `.rs`/`.ts`/`.py`/`.go` files
- **Knowledge index**: markdown heading-hierarchy + JSON/JSONL chunking over `docs/`, `openspec/`, `.omegon/`, `ai/memory/facts.jsonl`

Both indexes stored in `.omegon/codescan.db` (separate from `facts.db`). Lazy indexing on first query; incremental reindex on git HEAD change. BM25 ranking, no embeddings, no external search process.

Exposes two agent tools: `codebase_search` (query + scope) and `codebase_index` (invalidate).

## Motivation

Today the agent navigates code by grepping and reading full files, consuming 15-30k tokens to answer questions LSP or structured search could answer in <1k. The knowledge index adds a second layer: `docs/*.md` design decisions, `openspec/` specs, and `.omegon/` project state become searchable, giving the agent direct access to architectural context without multi-step memory_recall.

## Design node

See [codebase-search](../../docs/codebase-search.md) for full design decisions and research.
