# terminal.create@1 TDD Design

## Phase A — Policy validation before spawn

Tests first:
- command allowlist denial
- env deny-by-default
- env allowlist pass
- cwd outside allowed roots denied
- argv command vector generation has no shell string

Implementation:
- Add `terminal_create` policy helpers in `extensions/host_actions.rs` or a split module.
- Do not call PTY spawn until policy passes.

## Phase B — Terminal backend adapter

Tests first:
- unsupported when PTY runtime unavailable
- completed outcome shape when adapter reports a created session
- warnings/degradations appear in result

Implementation:
- Extract a safe internal function from `tools/terminal.rs` that starts a terminal from argv without shell interpolation.
- Keep existing terminal tool behavior intact.

## Phase C — Pipeline integration

Tests first:
- `terminal.create@1` policy path reaches executor only after manifest allows type and command policy passes
- `actions/execute` and declarative actions share the executor path
- headless unavailable path returns unsupported

Implementation:
- Replace Phase C executor-unavailable result for `terminal.create@1` with actual executor call.

## Risk controls

- No `bash -lc` for HostAction terminal creation.
- Default env is empty/deny.
- Reuse keys must include origin identity/session.
- Placement warnings are structured, not prose-only.
