# Terminal Backend Registry Tasks

## 1. Design and evidence capture
<!-- specs: extensions/terminal-backend-registry -->

- [x] 1.1 Capture Flynt terminal contract/manager evidence.
- [x] 1.2 Record that Flynt is a future backend candidate, not an Omegon dependency.
- [x] 1.3 Add issue #82 milestone/comment after implementation evidence.

## 2. Registry seam
<!-- specs: extensions/terminal-backend-registry -->

- [x] 2.1 Introduce terminal backend selection types around the existing HostAction executor.
- [x] 2.2 Move current portable PTY behavior behind a background backend implementation.
- [x] 2.3 Keep policy validation before backend selection/execution.

## 3. Placement behavior
<!-- specs: extensions/terminal-backend-registry -->

- [x] 3.1 Add side_pane -> background_session degradation warning when no visual backend exists.
- [x] 3.2 Add fake visual backend test proving side_pane is preferred when available.
- [x] 3.3 Preserve TerminalCreateResult wire shape.

## 4. Validation
<!-- specs: extensions/terminal-backend-registry -->

- [x] 4.1 Run `cargo test -p omegon terminal_create -- --nocapture`.
- [x] 4.2 Run `cargo test -p omegon extensions::tests:: -- --nocapture`.
- [x] 4.3 Run `cargo test -p omegon-extension -- --nocapture`.
- [x] 4.4 Run `just lint`.
