# Unified observation layer

## Intent

Implement A2 from `docs/harness-loop-guidance-affordances.md`: replace scattered raw tool-call parsing with one capability-aware observation normalizer that feeds harness guidance sensors consistently.

This fixes F9: research performed through `bash`, `grep`/`rg`, `sed`, `cat`, `view`, and search tools currently registers as zero file evidence because `IntentDocument::update_from_tools` only counts literal `read`/`understand` calls.

## Scope

- Add a unified `ObservationNormalizer` and semantic `ObservationEvent` model for completed tool calls.
- Derive non-bash tool classification from `ToolCapabilityCatalog` instead of hardcoded tool-name tables.
- Add conservative bash command classification for read/search/validation/progress-boundary observations.
- Route `IntentDocument::update_from_tools` through the observation stream for file-read, mutation, validation, and commit-boundary tracking.
- Preserve existing StuckDetector patterns and pressure behavior unless the new sensor data changes their inputs.
- Add focused regressions for F9 and existing behavior compatibility.

## Out of scope

- A1 task-mode policy.
- A3 novelty-rate/evidence ledger beyond event fields needed by A2.
- A4/A5 nudge wording or arbiter changes.
- A6/A7/A8 phase history, nudge outcome telemetry, and Pkl policy config.

## Success criteria

- `view`, `codebase_search`, and targeted capability-catalog tools contribute observation events without name-specific logic in `IntentDocument`.
- Successful bash read/search commands contribute evidence conservatively; failed commands do not.
- `git commit` through bash still clears mutation state as a progress boundary.
- Existing pressure-behavior tests remain green, with new F9 regressions pinned.
