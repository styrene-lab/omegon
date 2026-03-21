---
id: rust-secrets
title: Port secrets system to Rust (redaction, recipes, tool guards)
status: implemented
parent: ts-to-rust-migration
open_questions: []
---

# Port secrets system to Rust (redaction, recipes, tool guards)

## Overview

Port the 00-secrets extension: secret recipes (env, keychain, shell cmd), output redaction, tool guards for sensitive paths, audit log. Security-critical — must be in-process, not external.

## Decisions

### Decision: omegon-secrets crate with 4 layers, wired into dispatch_tools

**Status:** decided
**Rationale:** Separate crate for testability. Redaction at the dispatch level catches all tool output. Guards fire before dispatch. Recipes stored in ~/.omegon/secrets.json. 18 tests. Matches TS 00-secrets layers 1-4,6. Layer 5 (local model scrub) deferred — same mechanism applies.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon-secrets/` (new) — Full crate: 2,862 LoC, 8 modules (recipes, redact, guards, audit, resolve, store, vault, lib), 61 tests. SecretsManager facade wired into setup.rs + loop.rs.
- `core/crates/omegon/src/setup.rs` (modified) — SecretsManager initialized from ~/.omegon/, stored as Arc in AgentSetup
- `core/crates/omegon/src/loop.rs` (modified) — redact_content called on tool results, check_guard called before dispatch

### Constraints

- Redaction happens at dispatch level — catches all tool output before it enters conversation
- Guards fire before tool dispatch — blocks sensitive path access
- Encrypted store uses AES-256-GCM + Argon2id (separate from memory DB)
