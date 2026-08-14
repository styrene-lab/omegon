# Ecosystem Candidates

This is a research queue, not a dependency list. The resolver's **active** output is authoritative. Before adding a crate, verify compatibility with the project-resolved Ratatui/Crossterm versions, maintenance activity, feature/dependency cost, terminal ownership assumptions, accessibility, and testability.

## Already relevant when project-resolved

- **TachyonFX** — effects and animation scheduling. Confirm clipping, resize behavior, sendability, timer cost, and stable buffer testing.
- **ratatui-image** — terminal image protocols. Confirm primary/alternate behavior, resize cleanup, skipped-cell semantics, and text fallback.
- **ratatui-textarea** — multiline editing. Confirm wrapped cursor geometry, IME/paste behavior, undo bounds, and retained drafts.
- **ratatui-toaster** — transient notices. Confirm queue bounds, deduplication, priority, and exclusive-surface suppression.
- **tui-tree-widget** — hierarchical navigation. Confirm stable-key restoration, expansion persistence, and large-tree cost.
- **tui-popup** — compact popup geometry. Confirm it is not being used where explicit fullscreen ownership is required.
- **tui-syntax-highlight / syntect / ansi-to-tui** — rich text and code. Confirm hostile ANSI handling, input caps, cache strategy, theme mapping, and Unicode widths.
- **hyperrat** — inspect actual local usages and crate source before assigning a role.

## Research-in-advance categories

### Terminal capability discovery

Investigate maintained libraries or small project-owned probes for:

- color depth and `NO_COLOR`
- synchronized output
- keyboard enhancement
- hyperlink/OSC support
- image protocol support
- terminal identity and known quirks

Prefer evidence-backed capability records over TERM-name branching. Any probe must have a timeout and restore terminal modes.

### Unicode and text layout

Assess `unicode-width`, segmentation/line-breaking crates, bidi requirements, and terminal-width policy. The main risk is disagreement among wrapping, cursor placement, selection, and backend cell widths. One project-owned width policy should mediate libraries.

### Markdown and rich document rendering

Assess Ratatui markdown/rendering crates against the project's canonical markdown parser. Reject integrations that create a second semantic parser, emit unsafe controls, or cannot bound pathological input.

### Focus and component routing

Assess component/focus frameworks only if they preserve renderer-neutral actions and explicit return targets. Avoid frameworks that become a second application state model or hide terminal ownership.

### Forms, tables, and virtualized collections

Investigate stateful table/form helpers for large model, skill, extension, and settings inventories. Require stable IDs, explicit scroll state, virtualization, constrained-height tests, and accessibility-friendly keyboard paths.

### PTY and terminal integration testing

Investigate `portable-pty`, `expectrl`, or project-owned pseudoterminal harnesses for behavior that `TestBackend` cannot prove: alternate-screen sequences, resize, panic restoration, mouse modes, and native scrollback. Keep PTY tests narrow and deterministic.

### Observability and performance

Prefer existing tracing and scheduler instrumentation. Research frame-time histograms, allocation profiling, and dirty-region evidence before adding rendering caches or telemetry dependencies.

## Admission worksheet

For every candidate record:

1. Problem and observable requirement.
2. Why existing stack/project code is insufficient.
3. Exact resolved compatibility evidence.
4. Default features and transitive dependency cost.
5. Ownership/lifecycle assumptions.
6. Security and untrusted-input behavior.
7. Accessibility and fallback behavior.
8. Deterministic test strategy.
9. Removal/migration cost.
10. Decision: reject, defer, prototype, or admit.
