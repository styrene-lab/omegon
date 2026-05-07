+++
id = "b0a9b0d7-9c8a-43c1-9c58-8856528750e5"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# codebase_search — Tasks

## 1. core/Cargo.toml (modified)

- [ ] 1.1 Add `omegon-codescan` to workspace members list.

## 2. core/crates/omegon-codescan/ (new crate)

- [ ] 2.1 `Cargo.toml` — deps: `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-python`, `tree-sitter-go`, `rusqlite` (bundled), `pulldown-cmark`, `serde`, `serde_json`, `sha2`, `anyhow`, `tracing`, `glob`.
- [ ] 2.2 `src/lib.rs` — re-export `ScanCache`, `CodeScanner`, `KnowledgeScanner`, `BM25Index`, `Indexer`, `SearchChunk`, `SearchScope`, `IndexStats`.
- [ ] 2.3 `src/cache.rs` — `ScanCache::open(path) -> Self`; tables `code_chunks` and `knowledge_chunks` with `content_hash` column; `stale_paths(paths: &[PathBuf]) -> Vec<PathBuf>`; `upsert_code_chunks(path, hash, chunks)`; `upsert_knowledge_chunks(path, hash, chunks)`; `all_code_chunks() -> Vec<CodeChunk>`; `all_knowledge_chunks() -> Vec<KnowledgeChunk>`; `get_meta(key) / set_meta(key, val)` for HEAD tracking.
- [ ] 2.4 `src/bm25.rs` — `BM25Index::build(code: &[CodeChunk], knowledge: &[KnowledgeChunk]) -> Self`; `search(query: &str, scope: SearchScope, max: usize) -> Vec<SearchChunk>`; tokenise by splitting on whitespace and `[^a-z0-9]`; k1=1.5, b=0.75 standard BM25.
- [ ] 2.5 `src/code.rs` — `CodeScanner::scan_file(path: &Path, content: &str) -> Vec<CodeChunk>`; select grammar by extension; chunk at named declaration boundaries; text = full source text of the node; `item_kind` = "fn"/"struct"/"impl"/"enum"/"mod"/"class"/"interface".
- [ ] 2.6 `src/knowledge.rs` — `KnowledgeScanner::scan_markdown(path, content) -> Vec<KnowledgeChunk>`; use `pulldown-cmark` event stream to split at heading boundaries; extract YAML frontmatter (lines between `---` markers at file start) for `tags`; `KnowledgeScanner::scan_json(path, content) -> Vec<KnowledgeChunk>` (each top-level value as chunk, heading = "object N"); `KnowledgeScanner::scan_jsonl(path, content) -> Vec<KnowledgeChunk>` (each line); glob patterns for knowledge dirs configurable via `KnowledgeDirs` struct.
- [ ] 2.7 `src/indexer.rs` — `Indexer::run(repo_path: &Path, cache: &mut ScanCache) -> anyhow::Result<IndexStats>`; discover code files (walk `**/*.rs`, `**/*.ts`, `**/*.py`, `**/*.go`, skip `target/`, `node_modules/`, `.git/`); discover knowledge files (glob patterns from `KnowledgeDirs`); compute SHA-256 of each file content; compare against `cache.stale_paths()`; re-scan only stale files; write chunks to cache.
- [ ] 2.8 Unit tests: `cache.rs` — round-trip insert/query/invalidation; `bm25.rs` — single-term exact match scores higher than noise; `code.rs` — parse minimal Rust snippet (`fn foo() {}`) → 1 CodeChunk; `knowledge.rs` — parse markdown with 2 headings → 2 KnowledgeChunks with correct heading text.

## 3. core/crates/omegon/src/tools/codebase_search.rs (new)

- [ ] 3.1 `CodescanProvider` implementing `ToolProvider`: registers `codebase_search` and `codebase_index`.
- [ ] 3.2 `codebase_search` parameters: `query` (string, required), `scope` (string, default "all"), `max_results` (number, default 10), `tags` (array of string, optional).
- [ ] 3.3 On `codebase_search` execute: open (or create) `.omegon/codescan.db`; run `Indexer::run` for incremental update; build BM25Index from cached chunks; search; return markdown table with columns: File, Lines, Type, Score, Preview.
- [ ] 3.4 `codebase_index` parameters: `invalidate` (bool, default false). On execute: if invalidate, drop all chunks from cache; run `Indexer::run`; return `IndexStats` as formatted text.
- [ ] 3.5 Background git HEAD check: after first successful index, `tokio::spawn` a task that runs `git rev-parse HEAD` in repo_path, compares to `cache.get_meta("last_head")`, triggers `Indexer::run` if different.
- [ ] 3.6 Tests: `tool_definitions_registered` (2 tools with correct names); `execute_search_empty_corpus_returns_empty`; `execute_index_returns_stats`.

## 4. core/crates/omegon/src/tool_registry.rs (modified)

- [ ] 4.1 Add `pub mod codescan { pub const CODEBASE_SEARCH: &str = "codebase_search"; pub const CODEBASE_INDEX: &str = "codebase_index"; }`.
- [ ] 4.2 Add both constants to `all_static_names()`.
- [ ] 4.3 Update `TOOL_COUNT` from 51 to 53.

## 5. core/crates/omegon/src/tools/mod.rs (modified)

- [ ] 5.1 Add `pub mod codebase_search;`.
- [ ] 5.2 Register `CodescanProvider` in the tool builder/registration block (search for where other providers are added).

## 6. .gitignore (modified)

- [ ] 6.1 Add `.omegon/codescan.db` and `.omegon/codescan.db-*` entries.
