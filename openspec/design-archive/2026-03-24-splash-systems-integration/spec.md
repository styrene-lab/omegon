+++
id = "31ef9144-b8f1-4cfd-8160-1d336b383fd9"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Splash screen systems check visualization — real loading behind the animation — Design Spec (extracted)

> Auto-extracted from docs/splash-systems-integration.md at decide-time.

## Decisions

### Multi-line grid beneath logo — 3 columns, shows the breadth of startup work (decided)

A single line can only fit 3-4 items legibly. With 8-9 probe categories, a grid shows the true scope of what Omegon does at startup. The grid fills the vertical space between the logo and the 'press any key' prompt — space that's currently empty. Three columns of 3 rows is compact enough for the compact logo tier too. Each cell shows indicator + label + parenthetical summary when done. The visual effect of 9 items cascading from scanning to checkmark is significantly more impressive than 3 items blinking done.

## Research Summary

### Current splash architecture and what changes

**Current state** (`tui/splash.rs`):
- 3 hardcoded items: `providers`, `memory`, `tools`
- States: `Pending → Active → Done/Failed`
- Cosmetic cascade: providers done at frame 8, memory at 12, tools at 16
- No real work happens — items are set to Done at fixed frame thresholds
- Animation runs ~1.7s (38 frames × 45ms), then holds for dismissal
- Checklist renders as a single Line beneath the logo

**Proposed state**:
- Items reflect real async probes running on a tokio::spawn background task
- T…

### Implementation approach: async probes with channel feedback

The splash loop currently runs synchronously inside `run_tui`. To receive async probe results:

1. Before entering the splash loop, spawn a `tokio::spawn` task that runs all probes in parallel via `tokio::join!`
2. Each probe sends its result through an `mpsc::Sender<ProbeResult>` channel
3. The splash loop polls the channel each frame via `try_recv()` and updates item states
4. The `ProbeResult` enum carries both the item label and the result (done with summary, or failed with reason)

```rust
…
