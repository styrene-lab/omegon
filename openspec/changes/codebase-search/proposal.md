# codebase_search — AST-aware code retrieval with memory seeding

## Intent

A `codebase_search(query, strategy)` tool backed by tree-sitter AST parsing and BM25 keyword
indexing. Answers concept-retrieval questions ("find code about packet fragmentation") that LSP
cannot answer and that the agent currently handles by guessing file paths and running grep.

Inspired by ATLAS's PageIndex component (itigges22/ATLAS), which replaced Qdrant vector RAG with
AST-aware chunking after finding that function/class boundaries are semantically meaningful chunk
boundaries while arbitrary token windows are not.
