# LSP integration — language server protocol for code-aware navigation and diagnostics

## Intent

Use Language Server Protocol for structural code understanding — go-to-definition, find-references, diagnostics, symbols. Today the agent relies on grep/ripgrep for navigation. LSP gives it the same code intelligence a human IDE has: jump to definition, find all callers of a function, see type errors before running the compiler. OpenCode ships with native LSP; we have none.

See [design doc](../../../docs/lsp-integration.md).
