# Unified observation layer — Tasks

## 1. Observation normalizer
<!-- specs: harness-guidance/observation -->

- [x] 1.1 Add `core/crates/omegon/src/observation.rs` and `mod observation;`.
- [x] 1.2 Define `ObservationEvent` variants for file reads, mutations, searches, validations, and progress boundaries.
- [x] 1.3 Implement non-bash normalization from `ToolCapabilityCatalog` and successful `ToolResultEntry` matching.
- [x] 1.4 Implement conservative bash classification for read/search/validation/commit commands plus minimal mutation evidence from `touch`/`rm`/`mv`/`cp` and shell redirects.
- [x] 1.5 Add unit tests for capability-catalog read/search/mutation, failed and missing results, bash read/search/validation/commit/mutation, and unknown bash opacity.

## 2. IntentDocument integration
<!-- specs: harness-guidance/observation -->

- [x] 2.1 Replace hardcoded read/mutation/commit parsing in `IntentDocument::update_from_tools` with observation consumption.
- [x] 2.2 Preserve plan action handling in `IntentDocument::update_from_tools`.
- [x] 2.3 Preserve failed-approach tracking for error tool results.
- [x] 2.4 Add regression tests proving `view` and search-capable tools no longer hit the F9 blind spot.
- [x] 2.5 Add regression tests proving successful bash read and bash commit update guidance state correctly.

## 3. Compatibility validation
<!-- specs: harness-guidance/observation -->

- [x] 3.1 Run focused observation and conversation tests.
- [x] 3.2 Run existing pressure/behavior guidance tests affected by `files_read` / `files_modified`.
- [x] 3.3 Run `cargo check -p omegon --locked`.
- [x] 3.4 Update `CHANGELOG.md` under `[Unreleased]`.


## Implementation notes

- The normalizer distinguishes progress boundaries that clear mutation state from non-commit boundaries. Structured `commit` and successful `git commit`/`jj commit` through bash clear `files_modified`; other `ProgressBoundary` tools remain observable without falsely suppressing commit nudges.
- Positive observation evidence now requires a matching successful `ToolResultEntry`; missing results are treated as incomplete/opaque, not success.
- A1 currently implements the `Implementation` and `Research` policy rows. `QA` and `Maintenance` remain deferred until there are policy consumers for those modes.
- A2 intentionally keeps the minimal event schema needed by existing consumers. Novelty, search scope, and richer validation metadata are deferred to A3's evidence-ledger work.
- Non-bash classification is catalog-first, with legacy built-in name fallbacks retained as a compatibility boundary for existing tools.
