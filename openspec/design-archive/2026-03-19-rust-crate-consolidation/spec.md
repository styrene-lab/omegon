+++
id = "cc71c538-b6d8-4693-a8f9-8cb4b664b348"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Rust crate consolidation and ecosystem leverage — Design Spec (extracted)

> Auto-extracted from docs/rust-crate-consolidation.md at decide-time.

## Decisions

### Secrets crate upgraded to keyring+secrecy+aho-corasick; thin features kept separate (decided)

The secrets crate got the highest-value ecosystem upgrades: keyring for cross-platform credential storage (macOS+Linux+Windows), secrecy+zeroize for memory-safe secret handling, aho-corasick for single-pass redaction. The thin features (auto_compact, terminal_title, session_log) are well-structured with tests and not worth merging — clarity beats marginal registration savings. The omegon main crate is a 28K monolith but compiles fast (~6s) and has clear internal module boundaries — splitting would add complexity without clear reuse benefit yet.

## Research Summary

### Crate architecture audit

**Current state: 4 crates, ~33K LOC total**

| Crate | LOC | Purpose |
|-------|-----|---------|
| omegon | 28,679 | Main binary — everything |
| omegon-memory | 3,585 | Memory backend + 12 tools |
| omegon-secrets | 734 | Redaction, guards, recipes, audit |
| omegon-traits | 382 | ToolDefinition, ToolResult, ContentBlock |

**omegon main crate breakdown:**
- TUI: 7,116 LOC (25%) — ratatui widgets, dashboard, splash, editor
- Tools: 4,222 LOC (15%) — 14 tool files
- Features: 2,935 LOC (10%) — 9…

### Ecosystem leverage opportunities

**1. omegon-secrets → keyring + secrecy + aho-corasick**
Current: hand-rolled keychain shell-out, simple string::replace redaction, plaintext secret values in HashMap.
Upgrade path:
- `keyring` (v3) — cross-platform credential store (macOS Keychain, Windows Credential Manager, Linux Secret Service). Replaces the `security find-generic-password` shell-out in resolve.rs. Adds Windows/Linux support for free.
- `secrecy` + `zeroize` — wrap secret values in `Secret<String>` that auto-zeroes on drop. …
