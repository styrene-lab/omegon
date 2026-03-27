# LSP integration — language server protocol for code-aware navigation and diagnostics — Design Spec (extracted)

> Auto-extracted from docs/lsp-integration.md at decide-time.

## Decisions

### Auto-detect LSP servers from project files, with optional .omegon/lsp.toml override (decided)

validate.rs already does project-type auto-detection from Cargo.toml/tsconfig.json/requirements.txt and it works well. The LSP selection should follow the same pattern: Cargo.toml → rust-analyzer, tsconfig.json or package.json → typescript-language-server, go.mod → gopls, pyproject.toml/setup.py → pyright. An optional .omegon/lsp.toml allows operators to override the server path, add custom args, or add servers for additional languages. The default path must require zero configuration for the happy case (Rust projects get rust-analyzer without any setup).

### Build omegon-codescan (tree-sitter + BM25) before the LSP JSON-RPC client (decided)

Both lsp-integration and codebase-search need tree-sitter AST parsing. Factoring a shared omegon-codescan crate first avoids duplication and delivers codebase_search (discovery mode) at lower complexity than a full LSP client. codebase_search works on any project without external process requirements, while LSP requires language servers to be present. The stack is: omegon-codescan → codebase_search tool → LSP client → LSP navigation tools. Each layer adds value independently.

### MVP tool set is find_references + workspace_symbols + document_symbols first, goto_definition second, diagnostics third (decided)

find_references answers "where is this used?" — the most expensive question the agent currently answers by brute-force grep across files. workspace_symbols answers "where is anything named X?" without requiring the agent to know a file path first. document_symbols replaces full-file reads for understanding structure. goto_definition is precise but less frequently the bottleneck. diagnostics overlaps with validate.rs (post-mutation cargo check) but would give pre-mutation type checking — useful but a tier-2 priority.

## Research Summary

### Implementation approach — LSP client in Rust

OpenCode's approach: configure LSP servers per language in opencode.json (e.g. gopls for Go, rust-analyzer for Rust). The agent gets code intelligence via LSP responses.

For Omegon, the LSP integration would provide three new tools:
- `goto_definition(file, line, col)` → returns the definition location
- `find_references(file, line, col)` → returns all reference locations
- `diagnostics(file)` → returns compiler errors/warnings without running the build

The Rust crate `tower-lsp` provides LSP …

### OpenCode competitive re-assessment (March 2026)

**Full feature comparison — OpenCode vs Omegon (March 2026)**

### Current state audit — what the harness actually does for code navigation

Three tools handle code intelligence today:

**`validate.rs`** — already auto-detects project type from Cargo.toml/tsconfig.json/requirements.txt and runs cargo check/tsc/mypy after file mutations. This is the structural precedent for LSP auto-detection.

**`bash.rs`** — ripgrep/grep is the de facto navigation primitive. The agent builds context by: (1) guessing file paths, (2) reading full files, (3) grepping for symbol names. This is lossy — misses trait impls, macro-generated code, dynamic di…

### Sequencing recommendation — codebase_search before LSP client

The `codebase-search` node (exploring, P1) and this node share a key dependency: tree-sitter AST parsing. The right build order is:

**Step 1: `omegon-codescan` crate** — tree-sitter parsing + BM25 index
- No server processes, no JSON-RPC, no language-specific installation requirement
- Works immediately for any project, any language with a tree-sitter grammar
- Provides AST chunking for `codebase_search` (discovery mode) and the shared parsing layer for LSP
- Delivers `codebase_search` and `cod…

### Rust crate landscape — LSP client and tree-sitter options

Available crates for the implementation stack:

**LSP client:**
- `async-lsp-client 0.2.3` — async LSP client, most relevant
- `lsp-client 0.1.0` — simpler but minimal
- `lsp-types` — the de-facto types crate (used by most LSP crates including tower-lsp)
- No dominant high-quality async LSP client exists in crates.io; we'd likely wrap our own JSON-RPC stdio transport using `tokio::process::Command` + `tokio::io` (same pattern as dispatch_child in orchestrator)

**tree-sitter:**
- `tree-sitter 0.…
