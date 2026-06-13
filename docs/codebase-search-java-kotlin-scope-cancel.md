---
title: Codebase Search Java/Kotlin, Scoped Roots, and Cancellation
status: implemented
tags: [codescan, codebase-search, java, kotlin, cancellation, devex]
---

# Codebase Search Java/Kotlin, Scoped Roots, and Cancellation

## Problem

A remote operator report from a Java/Kotlin-adjacent workflow exposed three failures in `codebase_search`:

1. Java/Kotlin application code was not indexed, because `omegon-codescan` only discovered the earlier Rust/TypeScript/Python/Go-style language set.
2. The tool had no per-call path/root scoping parameter. If Omegon started at a broad workspace or home directory, `codebase_search` indexed too much and could return unrelated paths.
3. The `ToolProvider::execute` cancellation token was ignored, so an in-flight broad scan could not be interrupted by the operator.

## Current surface

Tool adapter:

- `core/crates/omegon/src/tools/codebase_search.rs`

Library crate:

- `core/crates/omegon-codescan/src/indexer.rs`
- `core/crates/omegon-codescan/src/code.rs`
- `core/crates/omegon-codescan/src/cache.rs`
- `core/crates/omegon-codescan/src/bm25.rs`
- `core/crates/omegon-codescan/src/knowledge.rs`

## Shipped decisions

### Regex Java/Kotlin/C# support is the first slice

Tree-sitter Java/Kotlin support remains desirable, but the 0.27.0-ready slice uses extension discovery plus regex chunking for JVM/.NET declarations. This makes Java/Kotlin/C# application source visible without adding dependency uncertainty.

Discovered extensions:

- `java`
- `kt`
- `kts`
- `cs`

Initial Java chunks:

- `class`
- `interface`
- `enum`
- `record`
- `@interface`
- methods / constructors approximated by declaration regex

Initial Kotlin chunks:

- `class`
- `interface`
- `object`
- `enum class`
- `sealed class`
- `data class`
- `fun`
- top-level `val` / `var` declarations

Initial C# chunks:

- `namespace`
- `class`
- `interface`
- `record`
- `struct`
- `enum`
- methods approximated by declaration regex

### Language logic stays bounded

Language-specific rules live in `core/crates/omegon-codescan/src/code/languages/`. `code.rs` owns dispatch and shared scanning engines only; new language support should add or modify a language module, not grow a central pattern list.

### `within` scopes returned results, not cache pruning

`codebase_search` now accepts a repo-relative `within` parameter. The adapter rejects empty paths, absolute paths, and `..` traversal, then verifies the resolved path stays under the provider root.

The first implementation deliberately filters loaded chunks/results instead of narrowing the indexer's discovery/pruning root. This avoids corrupting the shared `.omegon/codescan.db` by pruning full-root entries during a scoped search.

Scoped indexing remains a future optimization only if cache metadata is partitioned or pruning becomes prefix-aware.

### Cancellation is threaded through index and search

The tool adapter now passes `CancellationToken` through search and index execution. `omegon-codescan` exposes cancelable wrappers for the indexer and BM25 search loop while preserving the existing non-cancelled APIs for other callers.

Cancellation returns an explicit cancelled error instead of pretending there are no results.

### Operator diagnostics include scope context

Search details now report the requested `within`, provider root, and the filtered code/knowledge chunk counts used to build the BM25 index. This gives operators and clients enough context to understand why a scoped query did or did not return a file.

## Implemented checklist

1. Java/Kotlin/C# visibility
   - [x] update extension discovery in `indexer.rs`
   - [x] add Java/Kotlin/C# regex language modules
   - [x] add scanner tests for Java and Kotlin chunks
   - [x] add indexer discovery coverage through the codescan test suite
2. Scoped root
   - [x] add `within` schema to `codebase_search`
   - [x] add contained path resolver in the tool adapter
   - [x] filter returned code/knowledge chunks by repo-relative prefix
   - [x] add path traversal tests
   - [x] defer scoped cache pruning/indexing to a future cache-design slice
3. Cancellation
   - [x] add cancelable indexer/search APIs
   - [x] thread `CancellationToken` through provider execution
   - [x] add cancellation tests
4. Operator diagnostics
   - [x] include root / within / filtered chunk counts in search details
   - [x] include within context in no-results output

## Validation

Focused validation passed:

- `cargo test -p omegon-codescan -- --nocapture`
- `cargo test -p omegon codebase_search -- --nocapture`

## Acceptance criteria

- [x] Java and Kotlin source files are discovered by the indexer.
- [x] Java/Kotlin declarations produce searchable chunks.
- [x] The implementation has unit tests before adding tree-sitter dependencies.
- [x] Scoped-search work fails closed on path traversal.
- [x] Cancellation work stops broad scans before completion when cancellation is requested.
