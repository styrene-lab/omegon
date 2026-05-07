+++
id = "3728396a-5633-4a5a-9e14-84dc2de2489c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# cleave-checkpoint-parity — Design

## Spec-Derived Architecture

### cleave/preflight

- **cleave_run uses the same dirty-tree preflight as /cleave** (added) — 2 scenarios
- **volatile-only dirty trees are handled separately from substantive drift** (added) — 2 scenarios
- **project-memory avoids rewriting facts.jsonl when export content is unchanged** (added) — 1 scenarios
- **checkpoint approval uses a single structured confirmation flow** (added) — 2 scenarios
- **volatile-only policy default** (modified) — 1 scenarios
- **shared confirmation surface across execution modes** (modified) — 1 scenarios

## Scope

<!-- Define what is in scope and out of scope -->

## File Changes

<!-- Add file changes as you design the implementation -->
