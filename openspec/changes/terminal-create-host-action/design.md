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

## Async executor follow-up

The current production `terminal.create@1` backend bridges from the synchronous HostAction executor trait into the async PTY terminal adapter with `tokio::task::block_in_place` plus `Handle::block_on`. This is acceptable for the current multi-thread Omegon runtime, but it is intentionally documented as a future refactor point.

Future work should make the HostAction executor path async end-to-end so terminal creation can await directly and so current-thread runtimes or alternate hosts do not depend on `block_in_place` availability. Until then, this backend must remain bounded to Omegon runtime contexts that provide a Tokio runtime capable of `block_in_place`.
