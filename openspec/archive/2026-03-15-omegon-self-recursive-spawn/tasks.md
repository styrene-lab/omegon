+++
id = "7b398d3f-7c69-4f9e-af82-914ca7108734"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Recursive subprocesses must invoke Omegon-owned entrypoint, not bare `pi` — Tasks

## 1. extensions/lib/omegon-subprocess.ts (new)

<!-- specs: runtime/subprocess-entrypoint -->
- [x] 1.1 Add a shared helper that resolves the canonical Omegon subprocess contract as `process.execPath + bin/omegon.mjs`.
- [x] 1.2 Keep the helper independent of PATH lookup so internal recursion does not depend on a legacy `pi` alias winning shell resolution.

## 2. extensions/cleave/dispatcher.ts (modified)

<!-- specs: runtime/subprocess-entrypoint -->
- [x] 2.1 Replace bare `pi` child dispatch with the shared Omegon subprocess resolver.
- [x] 2.2 Preserve existing child flags, stdin prompt flow, and environment semantics while changing only executable resolution.

## 3. extensions/cleave/index.ts (modified)

<!-- specs: runtime/subprocess-entrypoint -->
- [x] 3.1 Route bridged spec-assessment subprocesses through the shared Omegon subprocess resolver.
- [x] 3.2 Route bridged design-assessment subprocesses through the shared Omegon subprocess resolver.
- [x] 3.3 Preserve the existing structured JSON assessment flow and non-interactive flags.

## 4. extensions/project-memory/extraction-v2.ts (modified)

<!-- specs: runtime/subprocess-entrypoint -->
- [x] 4.1 Route subprocess-based extraction fallback through the shared Omegon subprocess resolver.
- [x] 4.2 Preserve timeout, detached execution, and stdout/stderr handling semantics while changing only executable resolution.

## 5. Verification

<!-- specs: runtime/subprocess-entrypoint -->
- [x] 5.1 Confirm there are no remaining `spawn("pi", ...)` call sites under `extensions/` for the audited internal helper paths.
- [x] 5.2 Run typecheck and relevant extension tests to verify the refactor keeps current behavior intact.
