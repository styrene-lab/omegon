+++
id = "8e460d25-2f08-4a86-badf-abd490e1054f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# LSP integration — language server protocol for code-aware navigation and diagnostics

## Overview

Use Language Server Protocol for structural code understanding — go-to-definition, find-references, diagnostics, symbols. Today the agent relies on grep/ripgrep for navigation. LSP gives it the same code intelligence a human IDE has: jump to definition, find all callers of a function, see type errors before running the compiler. OpenCode ships with native LSP; we have none.

## Research

### Implementation approach — LSP client in Rust

OpenCode's approach: configure LSP servers per language in opencode.json (e.g. gopls for Go, rust-analyzer for Rust). The agent gets code intelligence via LSP responses.

For Omegon, the LSP integration would provide three new tools:
- `goto_definition(file, line, col)` → returns the definition location
- `find_references(file, line, col)` → returns all reference locations
- `diagnostics(file)` → returns compiler errors/warnings without running the build

The Rust crate `tower-lsp` provides LSP server infra but we need a client. The `lsp-types` crate gives us the protocol types. We'd spawn the appropriate LSP server (rust-analyzer, tsserver, gopls, pyright) as a subprocess and communicate via JSON-RPC over stdio.

Auto-detection: same pattern as project conventions detection in prompt.rs — if Cargo.toml exists, spawn rust-analyzer. If tsconfig.json, spawn tsserver. If go.mod, spawn gopls.

This is medium effort, high value. Every edit the agent makes could be validated structurally (not just syntactically) before committing.

### OpenCode competitive re-assessment (March 2026)

**Full feature comparison — OpenCode vs Omegon (March 2026)**

### Current state audit — what the harness actually does for code navigation

Three tools handle code intelligence today:

**`validate.rs`** — already auto-detects project type from Cargo.toml/tsconfig.json/requirements.txt and runs cargo check/tsc/mypy after file mutations. This is the structural precedent for LSP auto-detection.

**`bash.rs`** — ripgrep/grep is the de facto navigation primitive. The agent builds context by: (1) guessing file paths, (2) reading full files, (3) grepping for symbol names. This is lossy — misses trait impls, macro-generated code, dynamic dispatch, re-exports.

**`read.rs`** — reads files by path/offset. No structural awareness. The agent reads full files to understand 3 lines of relevant code.

**No `find_references`, `goto_definition`, `workspace_symbols`, or `document_symbols` exist.**

**The actual cost of the current pattern:**
- A typical "find all callers of function X" task: rg → 5-10 file reads → 15-30k tokens consumed to answer a question LSP can answer in <1k tokens (a list of file:line references).
- A "understand this type" task: read the declaration file → follow imports → read more files → 20-50k tokens vs hover/definition resolving in 1-2 round trips.

The validate.rs pattern is exactly what LSP server auto-detection should follow: detect project type → spawn appropriate server → cache the connection for the session.

### Sequencing recommendation — codebase_search before LSP client

The `codebase-search` node (exploring, P1) and this node share a key dependency: tree-sitter AST parsing. The right build order is:

**Step 1: `omegon-codescan` crate** — tree-sitter parsing + BM25 index
- No server processes, no JSON-RPC, no language-specific installation requirement
- Works immediately for any project, any language with a tree-sitter grammar
- Provides AST chunking for `codebase_search` (discovery mode) and the shared parsing layer for LSP
- Delivers `codebase_search` and `codebase_index` tools at low complexity

**Step 2: LSP client feature in `omegon`** — builds on codescan, adds JSON-RPC client
- Adds `goto_definition`, `find_references`, `diagnostics`, `workspace_symbols`, `document_symbols`
- Requires spawning language server processes (rust-analyzer, tsserver, etc.)
- Higher complexity, but language servers are already present in dev environments

**Why this order:**
- tree-sitter grammars are embeddable Rust crates — no external process needed
- BM25 over AST chunks solves 60% of the use cases (discovery/concept search) without any external dependency
- LSP adds the remaining 40% (precise navigation over known symbols)
- If we do LSP first, we still want tree-sitter for fallback when a language server isn't available

**MVP tool set for LSP:**
1. `find_references(file, line, col)` — highest agent value (callers, usages)
2. `workspace_symbols(query)` — fuzzy global symbol search without knowing file path
3. `document_symbols(file)` — structure of a file without reading its full content
4. `goto_definition(file, line, col)` — precise navigation to declaration
5. `diagnostics(file)` — type errors/warnings pre-compile (complements validate.rs which is post-compile)

workspace_symbols and document_symbols are higher value than goto_definition alone because they answer the discovery question ("where is anything called X?") that the agent hits constantly.

### Rust crate landscape — LSP client and tree-sitter options

Available crates for the implementation stack:

**LSP client:**
- `async-lsp-client 0.2.3` — async LSP client, most relevant
- `lsp-client 0.1.0` — simpler but minimal
- `lsp-types` — the de-facto types crate (used by most LSP crates including tower-lsp)
- No dominant high-quality async LSP client exists in crates.io; we'd likely wrap our own JSON-RPC stdio transport using `tokio::process::Command` + `tokio::io` (same pattern as dispatch_child in orchestrator)

**tree-sitter:**
- `tree-sitter 0.26.7` — stable Rust bindings
- `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-python`, `tree-sitter-go` — per-language grammar crates (all embeddable, compiled into the binary)
- `tree-sitter-grep 0.1.0` — structural grep via tree-sitter queries — potentially useful as a complement

**Key constraint:** `tree-sitter-rust` bundles a C parser that compiles at build time via `cc` in a build.rs. It's an embedding-safe dependency (no runtime download), but it adds C compilation to the build pipeline. This is a one-time build cost, not a runtime concern.

**Viable implementation pattern for LSP client:**
Spawn `rust-analyzer`, `typescript-language-server`, etc. as `tokio::process::Command` with stdin/stdout piped — exactly the same pattern used in `dispatch_child` in the cleave orchestrator. Read/write JSON-RPC messages on those streams. The protocol is simple enough to implement without a full client library (initialize → initialized → capabilities → textDocument/didOpen → workspace/symbol → shutdown).

**Language server availability assumption:**
For Rust (the primary use case): rust-analyzer is present in almost every Rust dev environment (`rustup component add rust-analyzer`). For TypeScript: `typescript-language-server` requires npm install. For Python: `pyright` requires pip. The auto-detect must check for server presence before attempting to spawn and degrade gracefully if absent.

## Decisions

### Decision: Auto-detect LSP servers from project files, with optional .omegon/lsp.toml override

**Status:** decided

**Rationale:** validate.rs already does project-type auto-detection from Cargo.toml/tsconfig.json/requirements.txt and it works well. The LSP selection should follow the same pattern: Cargo.toml → rust-analyzer, tsconfig.json or package.json → typescript-language-server, go.mod → gopls, pyproject.toml/setup.py → pyright. An optional .omegon/lsp.toml allows operators to override the server path, add custom args, or add servers for additional languages. The default path must require zero configuration for the happy case (Rust projects get rust-analyzer without any setup).

### Decision: Build omegon-codescan (tree-sitter + BM25) before the LSP JSON-RPC client

**Status:** decided

**Rationale:** Both lsp-integration and codebase-search need tree-sitter AST parsing. Factoring a shared omegon-codescan crate first avoids duplication and delivers codebase_search (discovery mode) at lower complexity than a full LSP client. codebase_search works on any project without external process requirements, while LSP requires language servers to be present. The stack is: omegon-codescan → codebase_search tool → LSP client → LSP navigation tools. Each layer adds value independently.

### Decision: MVP tool set is find_references + workspace_symbols + document_symbols first, goto_definition second, diagnostics third

**Status:** decided

**Rationale:** find_references answers "where is this used?" — the most expensive question the agent currently answers by brute-force grep across files. workspace_symbols answers "where is anything named X?" without requiring the agent to know a file path first. document_symbols replaces full-file reads for understanding structure. goto_definition is precise but less frequently the bottleneck. diagnostics overlaps with validate.rs (post-mutation cargo check) but would give pre-mutation type checking — useful but a tier-2 priority.

### Decision: With omegon-codescan and codebase_search shipped, LSP is the next code-intelligence precision layer rather than a prerequisite exploration

**Status:** decided

**Rationale:** The earlier sequencing decision has now paid off: omegon-codescan exists and codebase_search is implemented. That means the discovery layer is in place and LSP work can focus on precise symbol navigation and diagnostics, not on shared parsing infrastructure. The parent node no longer depends on speculative shared-code groundwork.

## Implementation Notes

### File Scope

- `core/crates/omegon-codescan/` (new)` — New shared crate — tree-sitter AST parsing, BM25 indexing, structural fact seeding. Shared dependency for both codebase_search and LSP.
- `core/crates/omegon/src/tools/lsp.rs` (new)` — LSP client feature — JSON-RPC over stdio to language servers. find_references, workspace_symbols, document_symbols, goto_definition, diagnostics tools.
- `core/crates/omegon/src/tools/codebase_search.rs` (new)` — codebase_search and codebase_index tools backed by omegon-codescan.
- `core/crates/omegon/src/tools/validate.rs` (modified)` — Existing post-mutation validator — auto-detection pattern is the LSP server selection model. Read-only reference.

### Constraints

- Language server processes must be spawned lazily per-session and cached — not per-tool-call.
- LSP initialization (textDocument/didOpen, workspace/symbol indexing) must happen in background without blocking the agent turn.
- tree-sitter grammars must be embedded (compiled into the binary) not downloaded at runtime.
- graceful fallback: if rust-analyzer is not installed, LSP tools return an actionable error rather than panicking.
- LSP tools must not assume a persistent daemon — each session starts fresh and server state is rebuilt.
- Token budget awareness: LSP tool results must be truncatable to a configured limit (find_references can return thousands of hits).
