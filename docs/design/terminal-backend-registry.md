+++
id = "terminal-backend-registry"
tags = ["extensions", "host-actions", "terminal", "reader", "flynt", "0.24", "issue-82"]
aliases = ["terminal-create-backend-registry", "issue-82-terminal-registry"]
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Terminal backend registry — Issue 82

## Overview

Issue #82 promotes `terminal.create@1` from a single local background PTY implementation into a host-side backend registry. Extensions such as `omegon-reader` continue to emit argv-only `terminal.create@1` intent; Omegon chooses the backend that can satisfy the requested placement and returns an honest `TerminalCreateResult`.

The immediate gap is reader UX: `reader_open` asks for `placement = side_pane`, but the current host implementation launches Bookokrat through `portable_pty` as `actual_placement = background_session`. That fallback is valid, but the host should report degradation and expose a registry seam for visual hosts.

## Flynt evidence captured

The Flynt sister checkout contains relevant terminal work:

- `49eccd9 feat(terminal): add reusable terminal session manager`
- `7635664 feat(terminal): define shared terminal contract types`
- `647ad73 refactor(terminal): remove spike terminology`
- `fd1ca15 refactor(terminal): isolate reusable terminal spike module`
- `c02bbe3 docs(design): record terminal HostAction alignment`
- `593a703 docs(terminal): align Flynt spike with Omegon terminal contract`
- `f3155ec feat(terminal): spike native alacritty terminal surface`

Relevant Flynt files:

- `/Users/wilson/workspace/styrene-labs/flynt/crates/flynt-app/src/terminal/types.rs`
- `/Users/wilson/workspace/styrene-labs/flynt/crates/flynt-app/src/terminal/manager.rs`
- `/Users/wilson/workspace/styrene-labs/flynt/crates/flynt-app/src/terminal/view.rs`
- `/Users/wilson/workspace/styrene-labs/flynt/docs/terminal-validation-actions.md`
- `/Users/wilson/workspace/styrene-labs/flynt/design/host-actions/flynt-host-actions-platform.md`

Important observed details:

- Flynt mirrors Omegon's `terminal.create@1` wire shape: `command`, `args`, `cwd`, `env`, `title`, `placement`, `reuse_key`.
- Flynt has `TerminalPlacement::{Default, SidePane, BottomPane, NewTab}` using snake_case JSON.
- Flynt's `TerminalCreateResult` matches Omegon shape: `terminal_id`, `backend`, `actual_placement`, `warnings`.
- Flynt has a reusable `TerminalManager` built on `portable-pty + alacritty_terminal` with stable IDs from `reuse_key`.
- Flynt explicitly says its terminal module should remain app-agnostic enough to extract for Auspex/shared Styrene terminal substrate, while Flynt-specific policy/UI/placement stays outside the module.

## Decisions

### Decision: Add a registry seam in Omegon before adding real Flynt integration

**Status:** decided
**Rationale:** Omegon should first route `terminal.create@1` through a backend registry with the existing portable PTY fallback. A test fake visual backend can prove side-pane selection without coupling Omegon to Flynt internals.

### Decision: Keep extension contract backend-agnostic

**Status:** decided
**Rationale:** `omegon-reader` should continue to emit argv-only intent. It should not know about Flynt, Zellij, Ghostty, Kitty, or ACP terminal hosts.

### Decision: Return honest degradation warnings

**Status:** decided
**Rationale:** If a request asks for `side_pane` and Omegon can only provide `background_session`, the result must say so explicitly through `warnings`.

### Decision: Flynt is an external visual host candidate, not a dependency

**Status:** decided
**Rationale:** Flynt's spike validates the target backend shape, but Omegon 0.24 should not depend on Flynt crates or renderer internals. A future integration can register a visual backend through the same registry seam.

## Implementation plan

1. Introduce internal terminal backend selection types near the HostAction terminal executor.
2. Move current `portable_pty` execution behind a `portable_pty` background backend.
3. Add registry selection logic that considers requested placement and backend capabilities.
4. Add degradation warnings for `side_pane` → `background_session` fallback.
5. Add tests with fake visual backend proving `side_pane` preference and policy-before-backend ordering.
6. Keep `TerminalCreateResult` wire shape unchanged.

## Non-goals

- No direct Flynt dependency in Omegon.
- No shared `terminal-core` crate extraction in this slice.
- No real ACP delegated terminal backend yet.
- No Zellij/Ghostty/Kitty backend behavior in this slice.
