---
title: Codebase Search Java/Kotlin, Scoped Roots, and Cancellation
status: implementing
tags: [codescan, codebase-search, java, kotlin, cancellation, devex]
---

# Codebase Search Java/Kotlin, Scoped Roots, and Cancellation

## Problem

A remote operator report from a Java/Kotlin-adjacent workflow exposed three failures in `codebase_search`:

1. Java/Kotlin application code is not indexed, because `omegon-codescan` only discovers `rs`, `ts`, `tsx`, `js`, `jsx`, `py`, and `go` files.
2. The tool has no per-call path/root scoping parameter. If Omegon starts at a broad workspace or home directory, `codebase_search` indexes too much and can return unrelated paths.
3. The `ToolProvider::execute` cancellation token is ignored, so an in-flight broad scan cannot be interrupted by the operator.

## Current surface

Tool adapter:

- `core/crates/omegon/src/tools/codebase_search.rs`

Library crate:

- `core/crates/omegon-codescan/src/indexer.rs`
- `core/crates/omegon-codescan/src/code.rs`
- `core/crates/omegon-codescan/src/cache.rs`
- `core/crates/omegon-codescan/src/bm25.rs`
- `core/crates/omegon-codescan/src/knowledge.rs`

## Decisions

### Start with regex Java/Kotlin support

Tree-sitter Java/Kotlin support is desirable, but the fastest safe first slice is extension discovery plus regex chunking for Java/Kotlin declarations. This immediately makes Java/Kotlin app source visible without adding dependency uncertainty.

Add discovered extensions:

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

### Keep language logic bounded

Language-specific rules live in `core/crates/omegon-codescan/src/code/languages/`. `code.rs` owns dispatch and shared scanning engines only; new language support should add or modify a language module, not grow a central pattern list.

### Add path scoping as a separate slice

Add a `within` parameter to `codebase_search` and `codebase_index` after Java/Kotlin support lands. The parameter must be repo-relative, canonicalized, and contained inside the provider root.

Open cache question: whether scoped indexing should share `.omegon/codescan.db` with full-root indexing. The safest initial behavior is to keep repo-relative paths and filter discovery by `within`; later we can partition cache metadata if stale pruning becomes surprising.

### Thread cancellation through index/search as a separate slice

The tool already receives `CancellationToken`; it must be checked before and during slow operations:

- before opening/running index
- while walking/discovering files
- while hashing/scanning files
- before loading chunks
- during BM25 scoring loops

Cancellation should return an explicit cancelled error/result, not `No results`.

## Implementation plan

1. Java/Kotlin visibility
   - update extension discovery in `indexer.rs`
   - add Java/Kotlin regex patterns in `code.rs`
   - add scanner tests for Java and Kotlin chunks
   - add indexer discovery test
2. Scoped root
   - add `within` schema to tool definitions
   - add contained path resolver in tool adapter or codescan crate
   - add indexer options/discovery root
   - add path traversal tests
3. Cancellation
   - add cancelable indexer/search APIs
   - thread `CancellationToken` through provider
   - add cancellation tests
4. Operator diagnostics
   - include indexed root / within / scanned files / duration in tool details
   - warn when root is broad or when hidden/cache directories are skipped

## Acceptance criteria

- Java and Kotlin source files are discovered by the indexer.
- Java/Kotlin declarations produce searchable chunks.
- The implementation has unit tests before adding tree-sitter dependencies.
- Later scoped-search work must fail closed on path traversal.
- Later cancellation work must stop broad scans before completion when cancellation is requested.
