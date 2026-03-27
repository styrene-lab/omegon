# codebase_search — AST-aware code retrieval with memory seeding — Tasks

## 1. core/Cargo.toml (modified)

- [ ] 1.1 Add `omegon-codescan` to workspace members list.

## 2. core/crates/omegon-codescan/ (new crate)

- [ ] 2.1 Scaffold `Cargo.toml` with deps: `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-python`, `tree-sitter-go`, `rusqlite`, `pulldown-cmark`, `serde`, `serde_json`, `sha2`.
- [ ] 2.2 `src/lib.rs` — re-export `ScanCache`, `CodeScanner`, `KnowledgeScanner`, `BM25Index`, `SearchChunk`, `SearchScope`.
- [ ] 2.3 `src/cache.rs` — SQLite-backed chunk cache at `.omegon/codescan.db`; tables: `code_chunks(path, start_line, end_line, item_name, item_kind, text, content_hash)` and `knowledge_chunks(path, heading, start_line, end_line, tags, text, content_hash)`; incremental invalidation by comparing `content_hash`; `ScanCache::open(db_path)`, `ScanCache::stale_files(paths)`, `ScanCache::upsert_code_chunks(...)`, `ScanCache::upsert_knowledge_chunks(...)`.
- [ ] 2.4 `src/bm25.rs` — in-process BM25 scoring (no Tantivy, no external process); tokenise by whitespace + split on punctuation; compute TF-IDF-style BM25 score over both chunk tables; `BM25Index::build(chunks)`, `BM25Index::search(query, max_results) -> Vec<SearchChunk>`.
- [ ] 2.5 `src/code.rs` — tree-sitter-based chunker; auto-select grammar from file extension (`.rs` → `tree-sitter-rust`, `.ts`/`.tsx` → `tree-sitter-typescript`, `.py` → `tree-sitter-python`, `.go` → `tree-sitter-go`); chunk at function/impl/struct/class/module boundaries; emit `CodeChunk { path, start_line, end_line, item_name, item_kind, text }`.
- [ ] 2.6 `src/knowledge.rs` — markdown heading-hierarchy chunker using `pulldown-cmark`; each `##`/`###` section is a chunk; extract YAML frontmatter (`id`, `title`, `status`, `tags`) from `---` blocks; JSON/JSONL flattener (each top-level object → one chunk); glob patterns for knowledge dirs: `docs/*.md`, `openspec/**/*.md`, `openspec/**/*.json`, `.omegon/*.json`, `ai/memory/facts.jsonl`; emit `KnowledgeChunk { path, heading, start_line, end_line, tags: Vec<String>, text }`.
- [ ] 2.7 `src/indexer.rs` — `Indexer::run(repo_path, cache)` walks code files and knowledge dirs; checks `cache.stale_files()` for incremental update; runs `CodeScanner` and `KnowledgeScanner`; writes chunks back to cache; returns `IndexStats { code_files, knowledge_files, code_chunks, knowledge_chunks, duration_ms }`.
- [ ] 2.8 Unit tests in each module: `cache.rs` — insert/query/invalidation round-trip; `bm25.rs` — basic scoring, empty query, single-term exact match; `code.rs` — parse a small Rust snippet into expected chunks; `knowledge.rs` — parse a design doc with frontmatter into expected sections.

## 3. core/crates/omegon/src/tools/codebase_search.rs (new)

- [ ] 3.1 Define `codebase_search` tool: `query: str` (required), `scope: "code" | "knowledge" | "all"` (default `"all"`), `max_results: int` (default 10), `tags: [str]` (optional, filters knowledge chunks by tag).
- [ ] 3.2 Define `codebase_index` tool: `invalidate: bool` (default `false`; when true, drops all cached chunks and re-indexes from scratch).
- [ ] 3.3 On first `codebase_search` call: open (or create) `.omegon/codescan.db`; run incremental indexer synchronously; then run BM25 search and return results.
- [ ] 3.4 Return format for each result chunk: `{ file, start_line, end_line, chunk_type: "code" | "knowledge", score, preview }` where `preview` is the first 800 chars of the chunk text, and `file` is repo-relative.
- [ ] 3.5 Return a formatted markdown table of results as the tool text content, plus structured JSON `details`.
- [ ] 3.6 Spawn a background `tokio::task` to check git HEAD and trigger incremental reindex when HEAD differs from last indexed HEAD (store last HEAD in codescan.db).
- [ ] 3.7 Tests: `execute_codebase_search_unknown_query_returns_empty`, `execute_codebase_index_returns_stats`, `tool_definitions_are_registered`.

## 4. core/crates/omegon/src/tool_registry.rs (modified)

- [ ] 4.1 Add `pub mod codescan { pub const CODEBASE_SEARCH: &str = "codebase_search"; pub const CODEBASE_INDEX: &str = "codebase_index"; }`.
- [ ] 4.2 Add both names to the `ALL_TOOL_NAMES` list (or equivalent registry constant) and update the `registry_count_is_current` test.

## 5. core/crates/omegon/src/tools/mod.rs (modified)

- [ ] 5.1 Register `codebase_search` and `codebase_index` tools in the tool registration block.

## 6. .gitignore (modified)

- [ ] 6.1 Add `.omegon/codescan.db` and `.omegon/codescan.db-*` to `.gitignore`.
