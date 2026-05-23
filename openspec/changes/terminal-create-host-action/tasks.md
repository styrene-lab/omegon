# terminal.create@1 Tasks

## 1. Policy validation before spawn
<!-- specs: extensions/terminal-create -->

- [x] 1.1 Add failing tests for command allowlist denial.
- [x] 1.2 Add failing tests for env deny-by-default and allowlist pass.
- [x] 1.3 Add failing tests for cwd root denial.
- [x] 1.4 Add failing tests proving argv command vectors are generated without shell strings.
- [x] 1.5 Implement terminal.create policy validation helpers.

## 2. Backend adapter
<!-- specs: extensions/terminal-create -->

- [x] 2.1 Add failing tests for unsupported PTY backend outcomes.
- [x] 2.2 Add failing tests for completed result shape from a fake adapter.
- [x] 2.3 Extract argv-based terminal start adapter from existing terminal machinery.
- [x] 2.4 Preserve existing terminal tool shell-string behavior only for the direct `terminal` tool.

## 3. Pipeline integration
<!-- specs: extensions/terminal-create -->

- [x] 3.1 Add failing tests proving declarative terminal.create reaches executor after policy.
- [x] 3.2 Add failing tests proving actions/execute reaches the same executor.
- [x] 3.3 Wire terminal.create executor into HostAction registry.
- [x] 3.4 Return terminal_id/backend/actual_placement/warnings in HostActionOutcome.result.

## 4. Validation and upstream closure
<!-- specs: extensions/terminal-create -->

- [x] 4.1 Run `cargo test -p omegon`.
- [x] 4.2 Run `cargo test -p omegon-extension`.
- [x] 4.3 Run `cargo check -p omegon`.
- [x] 4.4 Run `just link`.
- [x] 4.5 Post acceptance trace to #76 and close only after criteria map to tests/code.
