# Unified observation layer — Design

## Architecture decisions

### Decision: Add `observation` as a first-class loop module

Create `core/crates/omegon/src/observation.rs` with:

- `ObservationEvent`
- `ObservationNormalizer`
- conservative bash classifiers
- normalization tests independent of `ConversationState`

`main.rs` exposes it with `mod observation;` so guidance code can share the same sensor.

### Decision: Capability catalog is authoritative for non-bash tool classification

Non-bash tools are classified through `ToolCapabilityCatalog` and `ToolCapability` metadata. Name-specific fallbacks stay only where unavoidable for shell programs and legacy compatibility boundaries.

This keeps extension tools from needing bespoke guidance wiring when their tool definitions already declare inspection, mutation, or validation capability.

### Decision: Bash classification is conservative and evidence-positive only on success

Bash commands are parsed segment-by-segment across simple separators (`;`, `&&`, `||`, `|`, newline). The classifier recognizes:

- read programs: `cat`, `head`, `tail`, `sed`, `awk`, `wc`, `nl`, `strings`, `xxd`, `hexdump`
- search/list programs: `rg`, `grep`, `find`, `fd`, `ls`, `tree`
- validation programs: `cargo test/check/clippy/build`, `just test*/lint/check`, common package test commands
- progress boundary: `git commit`, `jj commit`

Unknown commands emit no event. Redirect targets are not counted as reads.

### Decision: A2 only rewires `IntentDocument` initially

The A2 implementation starts by routing `IntentDocument::update_from_tools` through normalized observations, because F9 is directly there and it is the lowest-risk integration point. Drift and StuckDetector can be moved onto the same stream in follow-up changes after the event model is proven.

## File scope

- `core/crates/omegon/src/observation.rs` — new observation event model, normalizer, bash classifier, tests.
- `core/crates/omegon/src/main.rs` — module declaration.
- `core/crates/omegon/src/conversation.rs` — replace hardcoded `IntentDocument::update_from_tools` read/mutation/commit parsing with normalized observations.
- `CHANGELOG.md` — Unreleased entry.

## Compatibility notes

- Existing `files_read` / `files_modified` fields remain the state surface for this stage.
- Existing commit and plan nudges remain intact.
- Failed tool results must not create positive evidence.
- The normalizer should be deterministic and allocation-light; full shell parsing is deliberately not attempted.
