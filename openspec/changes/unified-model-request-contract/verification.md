# Unified request preparation verification

## Ownership and provenance

- Branch: `feat/unified-model-request-preparation`.
- Base: `1828d6c5a6f78eceae9e811e9ba39e4ccf4f59d4`, the PR #246 merge.
- Worktree: `/Users/wilson/workspace/styrene-lab/omegon-model-request-preparation`.
- Proposal research was imported from the sibling `omegon-likec4` draft. The
  parent draft and diagram runtime remain owned by the parent session.
- Only `omegon` code changes. No traits, dependency, persisted-schema, credential
  resolution, or execution-generation changes are included.

## Scenario-to-test alignment

All names below are Rust unit tests in the owning crate. Tests use fake bridges,
local authority fixtures, or existing isolated memory owners.

| Delta scenario | Evidence |
| --- | --- |
| Rejected capability or evidence write | `model_request::tests::capability_and_evidence_failures_cannot_produce_dispatch_inputs`; `provider_route_service::tests::authority_append_failure_prevents_bridge_entry`; `lease_durability_failure_blocks_provider_dispatch` |
| Exact normal and repair inputs | `model_request::tests::retries_reuse_exact_inputs_and_one_evidence_receipt`; `loop_session::tests::turn_preparation_adapter_preserves_both_repair_identities_and_manifests`; `capture_preserves_exact_dispatch_inputs_and_event_backed_provenance`; `provider_route_service::tests::failed_attempt_chunks_remain_canonical_but_only_done_attempt_commits`; `lease_preserves_selected_and_serving_route_evidence` |
| Frozen tools and execution permission | `loop_session::tests::dispatch_capture_rejects_changed_schema_without_reusing_request_identity`; `schema_identity_is_canonical_and_changes_with_enabled_tools`; `loop_driver::tests::invocation_contract_selects_the_turn_tool_surface`; `bus::tests::disabled_tools_filtered_from_definitions`; `stale_generation_lease_is_rejected_before_owner_execution`; existing invocation lease tests |
| Invalid turn inputs | `provider_route_service::tests::prepared_turn_rejects_changed_inputs_and_owner_before_route_evidence`; `loop_context::tests::fixed_context_budget_stops_loop_before_provider_dispatch`; `loop_session::tests::full_spine_capture_rejects_unattributed_legacy_transcript_content` |
| Turn and idle evidence | `loop_session::tests::applied_compaction_context_matches_next_prepared_capture`; `session_compaction::tests::prepared_idle_summary_retains_session_authority_without_synthetic_turn`; `provider_route_service::tests::manifest_compaction_uses_native_identity_and_endpoint_evidence` |
| Oversized summary input | `provider_route_service::tests::prepared_compaction_preserves_input_authority_and_byte_limits` |
| Summary terminal failure | `provider_route_service::tests::prepared_compaction_rejects_all_unsuccessful_terminals` covers EOF, empty Done, Error, UpstreamFailure followed by Done, and a paused-clock idle timeout; successful cases use real turn/idle authority fixtures |
| Memory extraction acceptance | `provider_route_service::tests::prepared_auxiliary_retains_step_evidence_native_identity_and_budget`; `auxiliary_preparation_rejects_session_append_authority`; `providers::memory_completion_adversarial_tests`; `features::memory::formation::tests::wave3_classifies_candidates_and_rejects_fabricated_references`; `wave3_extraction_timeout_is_bounded` |
| Cancellation stale generation and overflow | `providers::memory_completion_adversarial_tests::adversarial_bounded_completion_rejects_oversized_utf8`; `adversarial_bounded_completion_rejects_eof_without_done`; `bounded_completion_rejects_error_before_done_and_legacy_keeps_eof`; `features::memory::formation::tests::cancelled_extraction_publishes_content_free_observation`; `wave3_capture_retains_goal_correction_and_attributed_outcome` includes stale capture rejection; existing memory cancellation/pending-source tests |
| Unbounded helper compatibility | `providers::memory_completion_adversarial_tests::bounded_completion_rejects_error_before_done_and_legacy_keeps_eof` |

The bounded helper retains `ModelExtractor`'s `MAX_EXTRACTION_BYTES` argument.
Memory's outer 30-second deadline and candidate parser are unchanged. Mutable
capture generation checks precede admission; stored immutable evidence and pinned
ended-session finalization retain their existing recovery semantics.

## Author validation record

Environment: `nix develop --offline --no-write-lock-file`, Rust/Cargo 1.95.0 from
the checked-in flake lock. Cargo uses `CARGO_NET_OFFLINE=true`; tests use
`RUST_TEST_THREADS=1` to respect process-global fixture isolation. An APFS clone of
the completed Vault worktree's target cache seeds this worktree's private `target/`.
The first cache-copy command used GNU `cp -c` and failed before any build; the
correct `/bin/cp -cR` succeeded. No dependency or toolchain installation occurred.

Focused gates completed:

- `just test-filter model_request::tests`: 3 passed.
- `just test-filter provider_route_service::tests`: 47 passed before the final
  auxiliary-owner regression was added.
- `just test-filter providers::memory_completion_adversarial_tests`: 4 passed.
- `just test-filter features::memory::formation::tests`: 6 passed.
- `loop_session::semantic_capture_tests` selected zero tests and is not validation
  evidence. The corrected `loop_session::tests` run exposed a test-only frontier
  assertion placed after route joining; the assertion now runs at capture time.
- Early route tests exposed incomplete legacy fixtures. Fixtures now send their
  captured messages and supply a response owner plus nonempty completed output.
- The first full crate run passed 5,363 tests and failed one source-boundary guard
  that expected inline capture in `loop.rs`. The guard now requires the typed
  `TurnRequestPreparation` adapter and retains its concrete-authority exclusions.
  All normal/repair, compaction, tool-surface, invocation-generation, and memory
  owner scenarios in that run passed. Clippy did not run after that test failure.
- The next full run passed all 5,365 unit tests, with 11 ignored, then failed the
  localhost instruction-discovery fixture with `WouldBlock`. Its isolated rerun
  passed both tests. The fixture now explicitly clears inherited nonblocking mode
  on accepted sockets before applying its existing read timeout. This test-only
  correction addresses the macOS socket race without changing request behavior.
  The final crate and Clippy gates passed with that correction.

Final gates completed successfully on the final Rust source:

```sh
nix develop --offline --no-write-lock-file -c sh -c \
  'export CARGO_NET_OFFLINE=true RUST_TEST_THREADS=1; just test-crate omegon && just clippy-changed'
```

- `just test-crate omegon`: passed. Unit tests: 5,365 passed, 11 ignored.
  Integration test executables: 26 reported passed, one PTY acceptance test ignored.
  The live-provider smoke suite remains opt-in; these totals do not claim a live
  provider run. The localhost CLI instruction fixture passed both tests.
- `just clippy-changed`: passed, including `cargo fmt --all --check` and
  `cargo clippy -p omegon --all-targets -- -D warnings`.
- Narrow `rustfmt --edition 2024 --config skip_children=true` formatting was used
  while editing. `git diff --check` passed.
- OpenSpec read-only `validate unified-model-request-contract` passed. The final
  task plan contains 12 completed tasks and remains in verification, without archival.

The main crate used its default `product`, `tui`, and `self-update` features on
macOS. No full-workspace or cross-platform gate is claimed.

## Review findings and remaining boundaries

Self-review found and addressed:

1. A durable request could otherwise pair a captured identity with changed input
   bytes at the route boundary. Read-only manifest comparison now rejects that pair.
2. Auxiliary preparation needed an explicit step-only evidence check. Supplying a
   session append owner now fails before any evidence write.
3. Compaction ignored structured upstream failure. It now records failure and
   returns immediately instead of accepting a later Done event.
4. The full-gate localhost fixture required explicit blocking mode on accepted
   sockets; its isolated test-only correction is documented in the design notes.

Independent code review completed with no blocking findings across the prepared
envelope, turn/repair capture, compaction, bounded auxiliary completion, and
scenario evidence. The reviewer passed `git diff --check` and OpenSpec structural
validation but did not rerun the author's full tests or Clippy gate.
The reviewer requested logical separation of the unrelated socket fixture fix.
That correction is committed separately as `0052d987`; request preparation and
its documentation form the second commit.

The parent user approved split commits and opening an implementation PR against
`main`. This approval does not authorize merge, final lifecycle acceptance, or
archival.

Cancellation, retries, source selection, invocation permissions, and generation
replacement remain with their existing owners. Receiver closure and owner-level
tests do not claim that every provider task is synchronously joined on future drop.
Unbounded helpers keep legacy EOF semantics. Other direct bridge callers are
outside the approved first scope.

The publication handoff covers two commits, a normal topic-branch push, and an
open PR. Artifact state follows tasks; tool-backed task/test registration and
parent diagram reconciliation remain pending owner review. No merge, install,
archive, or lifecycle-ledger mutation is authorized by this handoff.

Only factual review and handoff documentation changed after the successful author
gates. Final publication checks cover diff hygiene and OpenSpec validation;
unchanged Rust tests are not rerun for these documentation updates.
The parent diagram worktree, draft artifacts, session directory, shared LikeC4
server, and global configuration were not modified by this implementation.
