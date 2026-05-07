+++
id = "c87b9a9b-abb0-4d07-9aeb-0c4a9ce2ee78"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# codebase_search — Design

## Architecture Decisions

### Two-index SQLite cache

Two tables in `.omegon/codescan.db`:
- `code_chunks(path, start_line, end_line, item_name, item_kind, text, content_hash)`
- `knowledge_chunks(path, heading, start_line, end_line, tags, text, content_hash)`

Keyed by `(path, content_hash)` — changed files are re-chunked; unchanged files use cached chunks. Cache is separate from `facts.db` (different invalidation: time vs content).

### Code chunker (tree-sitter)

- Auto-select grammar from extension: `.rs` → `tree-sitter-rust`, `.ts/.tsx` → `tree-sitter-typescript`, `.py` → `tree-sitter-python`, `.go` → `tree-sitter-go`
- Chunk boundaries: `function_item`, `impl_item`, `struct_item`, `enum_item`, `mod_item` (Rust); `function_declaration`, `class_declaration` (TS/Go/Python equivalents)
- Grammars embedded at compile time (no runtime download)

### Knowledge chunker (markdown/JSON)

- Input patterns: `docs/*.md`, `openspec/**/*.md`, `openspec/**/*.json`, `.omegon/*.json`, `ai/memory/facts.jsonl`
- Markdown: `pulldown-cmark` splits by `##`/`###` headings; YAML frontmatter extracted for `id`/`title`/`status`/`tags`
- JSON: each top-level object → one chunk; JSONL: each line → one chunk

### BM25 ranking

- In-process, no Tantivy, no external process
- Tokenise by whitespace + punctuation split
- Unified ranking across both chunk tables for `scope=all`

### Lazy + incremental

- Index built on first `codebase_search` call, not at startup
- Background `tokio::task` checks `git rev-parse HEAD` after each query; triggers incremental reindex if HEAD changed since last index
- Last indexed HEAD stored in `codescan.db` metadata table

## Tool Interface

```
codebase_search(
  query: str,           // required
  scope: "code" | "knowledge" | "all",  // default "all"
  max_results: int,     // default 10
  tags: [str]           // optional knowledge-chunk tag filter
) -> markdown table of chunks

codebase_index(
  invalidate: bool      // default false; true = drop cache and full reindex
) -> IndexStats
```

Result chunk: `{ file, start_line, end_line, chunk_type, score, preview }` — 800-char preview max.

## File Scope

- `core/crates/omegon-codescan/` (new crate)
- `core/crates/omegon/src/tools/codebase_search.rs` (new)
- `core/crates/omegon/src/tool_registry.rs` (add codescan module, TOOL_COUNT → 53)
- `core/crates/omegon/src/tools/mod.rs` (register CodescanProvider)
- `core/Cargo.toml` (add omegon-codescan member)
- `.gitignore` (add .omegon/codescan.db*)

## Constraints

- tree-sitter grammars embedded at compile time (no runtime download)
- Knowledge dirs: `docs/`, `openspec/`, `.omegon/` (json/md), `ai/memory/facts.jsonl` — NOT `ai/memory/facts.db`
- BM25 in-process, no external dependency
- `codescan.db` not committed
- 263-file docs/ index must complete under 2 seconds
- Result previews: 300-char minimum, 1000-char maximum
