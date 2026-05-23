+++
id = "extension-side-process-backend-registry"
kind = "document"
title = "Extension Side-Process Backend Registry"
status = "seed"
tags = ["extension", "substrate", "backend", "zellij", "cockpit", "kitty"]
aliases = ["extension-side-process-backend-registry"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
parent = "extension-side-process-substrate-api"
dependencies = ["extension-side-process-substrate-api", "extension-side-process-capability-negotiation"]
open_questions = [
  "Where should substrate backends live: omegon core, separate crate, extension, or plugin?",
  "How should backend priority be configured?",
  "How should backend detection avoid expensive or blocking shell probes during TUI startup?",
  "What failure isolation is required when a backend command hangs or crashes?",
  "Can backend implementations share a common async trait without overfitting to one substrate?"
]
related = ["managed-reader-workspace", "cockpit-par-term-substrate-analysis", "reader-workspace-zellij-spike", "side-process-backend-terminal-compatibility-matrix"]
+++

# Extension Side-Process Backend Registry

## Overview

Design the core registry that maps extension side-process requests to concrete substrate backends.

The registry is the swappability seam. It lets Omegon ship one extension-facing API while testing Zellij, Cockpit/par-term, Kitty, or future substrates behind it.

## Candidate trait

```rust
#[async_trait]
trait SideProcessBackend {
    fn id(&self) -> BackendId;
    fn detect(&self) -> BackendStatus;
    fn capabilities(&self) -> CapabilitySet;

    async fn open(&self, request: ValidatedSideProcessRequest)
        -> Result<BackendOpenResult>;

    async fn close(&self, handle: BackendHandle) -> Result<()>;
    async fn status(&self, handle: BackendHandle) -> Result<BackendPaneStatus>;
}
```

This trait is illustrative. The real design should avoid promising close/status until a backend can actually provide them.

## Candidate backends

### Zellij backend

Role: v1 external managed workspace candidate.

Strengths:

- Real mux panes.
- Process isolation.
- Likely better for actual side-panel process UX.

Unknowns:

- Stable pane identity.
- Replace/close behavior.
- Graphics passthrough for Bookokrat PDF/image behavior.
- Managed install/bootstrap UX.

### Cockpit backend

Role: experimental embedded process-pane candidate.

Strengths:

- Already proven with `vi` and Bookokrat EPUB text smoke tests.
- No external mux requirement.
- Can live inside Omegon's own Ratatui layout.

Unknowns:

- Mouse routing.
- Graphics/image behavior.
- API stability.
- Event-loop integration complexity.

### Kitty backend

Role: optional terminal-specific spike.

Strengths:

- Strong terminal graphics support.
- Side-window/process orchestration may be possible in a Kitty-native way.

Unknowns:

- Kitty-specific product branch.
- Remote-control security and setup.
- Ghostty parity.

### Fallback backend

Role: structured failure with setup instructions.

It does not open panes. It reports why the request cannot be satisfied and what the operator can install/enable.

## Detection policy

Backend detection should be:

- cached;
- timeout-bounded;
- non-interactive;
- explicit about degraded/unknown states;
- safe to run in TUI context.

## Research tasks

1. Define backend status enum.
2. Define priority and selection rules.
3. Draft capability map for Zellij/Cockpit/Kitty/fallback.
4. Identify which backend operations require async process spawning.
5. Define timeout and cancellation behavior.
6. Decide whether backends are compiled into core or loaded as internal plugins.
