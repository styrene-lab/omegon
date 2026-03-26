---
description: Demo recording — styrene-rs codebase exploration and parallel Cleave execution
---
# Styrene-rs Demo

You are in a Rust workspace implementing the Reticulum Network Stack (RNS) and LXMF messaging protocol. Do the following two tasks in sequence.

## 1. Architectural Snapshot

Read `crates/libs/styrene-rns/Cargo.toml` and `crates/libs/styrene-lxmf/Cargo.toml`.

Explain in 3–4 sentences: what does each crate own, how do they depend on each other, and where does `styrene-tunnel` fit in that stack? Cite the specific dependency declarations you find.

## 2. Parallel Annotation Across Three Crates

Add `#[must_use]` annotations to every public function that returns `Result` across three crates:

- `crates/libs/styrene-rns/src/`
- `crates/libs/styrene-lxmf/src/`
- `crates/libs/styrene-tunnel/src/`

Each crate is independent — run this as a Cleave task so all three are modified in parallel branches and merged automatically.
