# TUI integration testing — snapshot tests, PTY harness, and interactive verification

## Intent

The TUI is 7,926 LoC across 14 files with 124 tests, but most tests are logic-only (conversation state, scroll math, data structures). Only dashboard.rs uses TestBackend for actual render verification, and those tests check for text presence, not visual layout. No snapshot testing. No interactive testing. No PTY-based integration tests.\n\nWith the TUI surface pass adding dashboard harness section, context class selector, toast notifications, and compaction indicators, we need a testing strategy that catches visual regressions — not just logic bugs.

See [design doc](../../../docs/tui-integration-testing.md).
