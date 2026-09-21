# Implementation tasks

## 1. Startup and defaults
<!-- specs: provider-onboarding -->
- [x] Add failing tests for absent defaults, CLI/profile precedence, and unselected persistence.
- [x] Remove hardcoded startup model, use registry provider defaults, and preserve explicit intent.
- [x] Verify empty startup never probes or falls back to a fabricated provider.

## 2. Disconnected composer and connection flow
<!-- specs: provider-onboarding -->
- [x] Add failing render and submission tests for disconnected, expired, and canceled setup.
- [x] Preserve drafts/attachments and present compact connection choices without false telemetry.
- [x] Integrate explicit free hosted choices with provider/data-policy labels.

## 3. Anonymous Zen provider
<!-- specs: provider-onboarding -->
- [x] Add failing catalog eligibility, deadline/failure, withdrawal, and no-paid-fallback tests.
- [x] Register Zen in shared provider admission with bounded catalog discovery and reviewed free models.
- [x] Verify ordinary streaming/tool calls and actionable throttling through a local fixture.

## 4. Acceptance and handoff
<!-- specs: provider-onboarding -->
- [x] Capture unconfigured inline/fullscreen startup and draft-preserving setup using private PTYs.
- [x] Review the integrated changes and run crate, Clippy, and applicable script gates.
- [x] Update Unreleased, record scenario evidence, validate OpenSpec, and commit focused changes.

- [x] Isolate the existing user-profile capture test after detecting a write to the operator home; verify profile bytes or absence are unchanged.
