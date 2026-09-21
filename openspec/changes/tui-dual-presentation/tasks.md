# Implementation tasks

Task wording was refined during implementation to reuse existing suites and record
actual test owners rather than require duplicate tests under proposed names. Red
assertions and native/PTY discoveries are recorded in verification.md. Checked
items include their focused validation; final landing work is tracked separately.

## 1. Independent preferences
<!-- specs: tui-presentation -->

- [x] Add red/green coverage for legacy density, launcher marker/literal arguments, preference explicitness and Ctrl+G migration.
- [x] Resolve entry/profile/CLI axes independently; expose global --tui/--ui flags; preserve absent and explicitly saved preferences across unrelated saves.
- [x] Extend UI routing, status and menu controls; base switches remain session-local and mounted roots retain fullscreen until closed.
- [x] Enable om inline/Active and omegon fullscreen/Full after the first successful PTY/native slice; verify actual fixed-build launcher defaults and all four combinations.

## 2. One terminal owner and shared rendering
<!-- specs: tui-presentation, tui-inline-publication -->

- [x] Add active-surface I/O validation and two coordinated Ratatui buffers under the existing terminal mode ledger; reject inactive writes.
- [x] Exercise acquisition/release failure for every tracked mode and retained-mode cleanup; review terminal creation/geometry errors propagating to the same guard without another rollback ledger.
- [x] Reuse frame preparation, composer, input/navigation precedence and rich widgets. Decisions and file pickers borrow the complete shared fullscreen widget.
- [x] Verify nonzero-origin geometry at 40/56/90 columns and short heights for both detail levels, multiline Unicode drafts, covered decisions, cancellation and second-turn recovery for both bases.
- [x] Keep primary history at inline startup and across repeated visits; defer splash/image probing; re-anchor after explicit primary output and restore output attributes on exit.

## 3. Bounded publication and authoritative lifecycle
<!-- specs: tui-inline-publication, tui-presentation -->

- [x] Implement canonical generation/index/field/byte cursors, incremental Active outcome reduction, bounded Full evidence, and mutable live previews without a duplicate transcript.
- [x] Verify byte/record/row/cell limits, injected cooperative clock, large-prefix skipping, oversized Unicode, zero geometry and partial-record continuation across resize/detail changes.
- [x] Commit only after guarded insert/flush success; preserve known non-writes, reject stale settlement and disable replay after ambiguous output. Verify terminal-control sanitization.
- [x] Separate explicit snapshot export from automatic delivery; preserve cursor and re-anchor geometry; keep exit draining bounded and identify unprinted output.
- [x] Fix the captured active Ctrl+C failure by polling priority ingress during the active supervisor wait, retaining durable admission and the cancellation deadline.
- [x] Fix the captured /new publication failure by invalidating generation at both source-replacement owners before subsequent events arrive; publish bounded resume/replacement notices.
- [x] Finalize cancelled/failed/timed-out supervisor outcomes, show their truthful terminal notice once, and retain existing lost-AgentEnd/idle-queue recovery.
- [x] Reproduce the release first-turn route race; admit local manifests/cached evidence before background discovery, pass inference-runtime tests and the four-request headless fixture.

## 4. Automated terminal evidence
<!-- specs: tui-presentation, tui-inline-publication -->

- [x] Extend the fixture and kit to record both axes, actual launcher entry, frozen binary/support hashes, shell-history markers, exactly-once replies and clean exit.
- [x] Capture the normal four-request sequence and the six-request stress sequence with gated large streaming output, unsent draft, browsing, denial, cancellation, reset and successful subsequent turn.
- [x] Run all four combinations in the PTY and both defaults in the five installed native clients; cover mixed combinations in Ghostty. Inspect attributable screenshots and distinguish native input limitations.
- [x] Extend and run real PTY detachment during idle/active work for both bases; answer the bare fixture's cursor query without claiming it is a terminal emulator.
- [x] Address operator disruption: make GUI trials explicitly opt-in with selected clients, attempt ownership-checked window closure in finally, verify its result, and test cleanup/opt-in contracts headlessly. No further GUI launches are authorized for routine iteration.

## 5. Documentation and landing
<!-- specs: tui-presentation, tui-inline-publication -->

- [x] Update Pkl vocabulary, public controls/migration docs, root directives and Unreleased. Record decorative telemetry retirement as a separate planned change.
- [x] Finish serialized crate/Clippy/script/schema checks, copy final evidence and inspect the complete diff.
- [x] Reconcile this change and parent pending items, create logical commits, and rebuild/install the current launcher pair without launching GUI windows; record the catalog/home-identity blocker separately.
- [x] Verify scenarios against the recorded evidence and mark completion truthfully. Leave archival and the separately planned telemetry retirement open.

## 6. Persistent notification follow-up
<!-- specs: tui-inline-publication -->

- [x] Reproduce hidden startup, control-response and local notifications after an already-published system segment; make persistent notices append centrally while preserving explicit mutable plan snapshots.
- [x] Verify notification retention rollover, partially published retained records and stale-batch rejection; preserve bounded memory and no replay.
- [x] Repeat crate/lint checks and private PTY startup, status and clean-exit acceptance against the identified build.

## 7. Readable live inline output
<!-- specs: tui-inline-publication -->

- [x] Reproduce long streamed replies disappearing from the small live viewport while primary history has no answer prefix; retain failing private PTY evidence.
- [x] Publish stable grapheme-safe assistant prefixes during streaming, keep only the unfinished tail live, and drain eligible backlog within existing input/publication budgets.
- [x] Preserve ordered completed tool-run summaries and Full thinking evidence, completion/cancel deduplication, generation/pruning behavior, resize/detail changes, and fullscreen round trips.
- [x] Verify numbered long responses in primary history before completion, continued output, tool interleaving, Unicode wrapping, cancellation and a second turn with the identified build; run Rust/script gates and record captured acceptance evidence.

Record the installed operator handoff in the external evidence directory after
committing the tested source, so the release artifact and launcher identify the
same revision. That record includes window/process identity and owned-session cleanup.

## 8. Readable Markdown and prose wrapping
<!-- specs: tui-inline-publication -->

- [x] Reproduce raw Markdown and mid-word wrapping in unit tests and a captured local-provider run; retain the failing artifact and observations.
- [x] Preserve shared Markdown styles and context through bounded native publication and its live tail; wrap prose at word boundaries with code/list/table structure intact.
- [x] Verify chunk-boundary, Unicode, budget, resize, completion, cancellation, and second-turn regressions; inspect styled terminal captures before completion.
- [x] Complete crate and Clippy gates, adversarial review, and specification reconciliation; record the installed operator handoff externally after committing the tested source as described above.

## 9. Uninterrupted response flow
<!-- specs: tui-inline-publication -->

- [x] Reproduce status appearing between published text and the live tail, then move transient status into the composer frame with completion/backlog coverage.
- [x] Verify held terminal captures, run crate/Clippy gates, and record the installed in-place operator handoff externally after commit.
