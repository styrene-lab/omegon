# Implementation tasks

The original implementation and acceptance are recorded in verification.md. Operator feedback exposed the connection-first follow-up below.

## 1. Quiet startup and failure guidance
<!-- specs: provider-connections -->

- [x] Add failing route-summary tests and make catalog growth irrelevant through route-only projection inputs; cover missing selection, expired credentials, failed bridges and fallback, with both layouts/detail levels exercised during acceptance.
- [x] Introduce compact interactive startup policy in bootstrap_projection/setup; consolidate duplicate startup credential warnings while preserving explicit control/ACP diagnostics.
- [x] Add a failing NullBridge response test; replace suggested-provider enumeration with concise /connect guidance.
- [x] Verify output remains bounded as provider inventory grows and update only affected snapshots.

## 2. Shared connection menu and command migration
<!-- specs: provider-connections -->

- [x] Add failing tests for Connections/Add provider grouping, expired and external credentials, empty state, filtering, and route badges using existing status projections.
- [x] Adapt auth_menu_projection and App menu entry to the two views; reuse shared menu interaction and inline terminal ownership.
- [x] Add failing /connect dispatch and registry/control authorization tests, including unknown providers and unsupported secure remote interaction.
- [x] Register /connect and route direct setup through existing auth handlers; preserve /login and /auth login compatibility and internal protocol identifiers.
- [x] Add secret-input and cancellation regressions; separate explicit API-key console opening from setup and verify browse/search cannot open a browser.
- [x] Update auth/route/main/footer remediation, command help, and relevant public documentation to prefer /connect; retain /model and /logout semantics.

## 3. Captured acceptance and handoff
<!-- specs: provider-connections -->

- [x] Extend isolated headless PTY acceptance for quiet startup, connection discovery, search, cancel/draft restoration, and one stubbed credential flow in both layouts and detail levels; record build identity and inspect captures.
- [x] Run the broader just test-rust and just lint gates because wait correlation changes the shared event contract; run affected script/site checks and record actual results and limitations.
- [x] Update Unreleased for implemented behavior, verify every scenario, and reconcile OpenSpec tasks with evidence. Keep the future /login renewal proposal separate.

## 4. Adversarial review and inherited integration corrections
<!-- specs: tui-review-regressions -->

- [x] Reproduce and fix WezTerm double-removal of its temporary resize pane with headless mocked trial and failure-retention tests; keep the correction in its own commit.
- [x] Reproduce and fix abandoned active/queued runtime decisions on authoritative completion, idle, and session reset; preserve drafts/menus and ignore advisory/stale completion.
- [x] Correlate operator-wait completion with its tool-call identity so timeout promotes the next queued decision and stale completion cannot dismiss a newer wait; preserve external wire behavior.
- [x] Obtain independent rereview of provider connections and integration corrections, run final current-source gates, and describe inherited parity/TUI scope accurately in the PR against main.

## 5. Connection-first operator feedback
<!-- specs: provider-connections -->

- [x] Reproduce provider Enter opening a model prompt; make connection setup primary and expose supported authentication methods without side effects on browse or cancel.
- [x] Replace Rust debug route summaries with actionable operator language; retain explicit diagnostic intent and state.
- [x] Verify actual /connect, provider selection, method selection, and cancel in private PTYs; run crate and Clippy gates. Final installed operator handoff is recorded externally after the source commit.
