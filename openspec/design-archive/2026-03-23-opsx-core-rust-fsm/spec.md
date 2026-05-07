+++
id = "1333f0b1-4031-4e0b-a800-a46acc164a55"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# opsx-core — Rust-backed lifecycle FSM for OpenSpec enforcement — Design Spec (extracted)

> Auto-extracted from docs/opsx-core-rust-fsm.md at decide-time.

## Decisions

### JSON files in repo as state store, not sled — jj/git IS the transaction log (exploring)

sled adds opacity and a dependency. Structured JSON files in .omegon/lifecycle/ are transparent, diffable, mergeable, and versioned by jj/git for free. The VCS operation log becomes the audit trail. sled is reserved for Omega's fleet-scale ACID requirements. Single-operator Omegon doesn't need an embedded database when the filesystem + VCS already provides persistence, transactions (commits), and conflict resolution (jj conflicts).

### Shared library crate — both Omegon and Omega depend on opsx-core (decided)

Omegon uses opsx-core with JSON file backend (single operator, git-native). Omega uses opsx-core with sled backend (fleet, ACID). The FSM logic, type definitions, and validation are the same — only the storage backend differs. This is a classic trait-based abstraction: trait StateStore with JsonFileStore and SledStore implementations.

### One-way: JSON state → generated markdown. Operator edits go through FSM commands, not direct markdown editing. (decided)

Bidirectional sync is a complexity trap. The JSON files are the source of truth. Markdown is generated/regenerated on state changes. If an operator edits markdown directly, the next FSM operation overwrites it. This is the same contract as generated code — edit the generator input, not the output. jj/git shows the conflict if someone edits markdown while the FSM also updates it.

## Research Summary

### jj/git compatibility and synergy


