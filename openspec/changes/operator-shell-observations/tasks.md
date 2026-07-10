+++
title = "Operator Shell Observations Tasks"
tags = ["openspec","tasks","shell","conversation"]
+++

# Operator Shell Observations Tasks

# Operator shell observations — Tasks

Dependencies: Group 1 defines the canonical contract. Groups 2 and 3 depend on Group 1 and may proceed independently after it. Group 4 depends on all implementation groups.

## 1. Canonical observation model and provider projection
<!-- specs: conversation/operator-shell-observations -->

- [ ] 1.1 Add the provenance-bearing operator tool observation type and canonical `AgentMessage` variant in `core/crates/omegon/src/conversation.rs`.
- [ ] 1.2 Add current and decayed LLM projections as clearly attributed user-role evidence, with terminal-control sanitization and bounded output.
- [ ] 1.3 Prove provider-shape repair does not fabricate or emit orphaned assistant tool-call/result pairs.
- [ ] 1.4 Add conversation tests for success, non-zero exit, provenance, decay, and role alternation.

## 2. Runtime commit and persistence
<!-- specs: conversation/operator-shell-observations -->

- [ ] 2.1 Add a single-owner completion path from spawned `RunShellCommand` execution back to `InteractiveAgentState` in `core/crates/omegon/src/main.rs`.
- [ ] 2.2 Commit command, cwd, exit status, duration, result, and `bang_shell` origin after execution completes while retaining streaming events.
- [ ] 2.3 Extend session snapshot conversion in the session persistence files to round-trip operator observations with backward-compatible defaults.
- [ ] 2.4 Add runtime and session regression tests showing the next turn and restored sessions retain operator-run evidence.

## 3. Semantic provenance and terminal rendering
<!-- specs: conversation/operator-shell-observations -->

- [ ] 3.1 Carry explicit execution origin through tool events and shared semantic conversation projections; update TUI, ACP, WebSocket, and audit adapters without id-prefix inference.
- [ ] 3.2 Extract the ANSI-aware terminal output helper from `core/crates/omegon/src/tui/segment_components/tool_card.rs` and reuse it for live and completed output.
- [ ] 3.3 Stop applying Bash source syntax highlighting to Bash stdout/stderr while retaining source semantics for command arguments.
- [ ] 3.4 Add renderer tests for ANSI style retention, malformed-control sanitization, plaintext neutrality, and live/completed parity.

## 4. Verification and release memory
<!-- specs: conversation/operator-shell-observations -->

- [ ] 4.1 Run focused conversation, runtime, session, surface, and TUI tests.
- [ ] 4.2 Run `cargo test -p omegon --locked`, `just lint`, and `just test-rust`.
- [ ] 4.3 Update `[Unreleased]` in `CHANGELOG.md` with operator shell persistence/model visibility and terminal rendering behavior.
- [ ] 4.4 Run `just link`, reconcile OpenSpec/design-tree state, and commit with a conventional commit message.
