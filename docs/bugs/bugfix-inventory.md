+++
title = "Bugfix inventory"
tags = ["bugs", "backlog", "providers", "credentials", "tui"]
+++

# Bugfix inventory

Confirmed or operator-observed defects deferred from the main Omegon work. Entries record the observed behavior, expected behavior, and investigation scope without prematurely prescribing an implementation.

## Text-policy continuation omits provider usage from run totals

**Type:** Bugfix

**Status:** Confirmed; not fixed

**Target:** Before 1.0

**Surface:** Agent-loop usage events and headless run summaries

### Observed failure

Provider-reported usage reaches the durable response receipt correctly. However, the
`text_policy_continuation` branch continues before emitting `TurnEnd`. The headless
run summary sums usage from `TurnEnd` events and therefore omits that response.
This is an aggregation defect, not a provider tokenization or estimation error.

The unified-prompt authorized-workflow probe recorded 26,641 tokens across six
provider responses, but its run summary reported 22,502. The omitted response
contained 4,092 input tokens and 47 output tokens: a 4,139-token undercount.
The subsequent continuation response was counted.

Source owners: `core/crates/omegon/src/loop.rs` (`text_only_recovery` continuation),
`core/crates/omegon/src/main.rs` (`TurnEnd` usage accumulation), and
`core/crates/omegon/src/provider_route_service.rs` (response receipts).
Private reproduction evidence is retained under
`.git/dev-terminal/unified-prompt-live-u0lgoi_l/04-authorized-workflow/`;
that directory is not part of the distributed repository.

### Acceptance criteria

- Account for each completed provider response exactly once, including responses
  that trigger text-policy continuation, independently of turn-completion events.
- Add a regression that reconciles reported input and output totals with provider
  usage receipts across ordinary completion and text-policy continuation.
- Confirm the triggering response and subsequent response are both counted,
  without duplication.
- Check other usage consumers for the same event dependency before declaring
  their totals correct. The observed failure establishes the headless-run defect,
  not the scope of every session or UI counter.

Tracking only; implementation is deferred to the pre-1.0 fix.

## Assignable designs

- [Inactive-provider credential expiry notifications](inactive-provider-credential-expiry-notifications.md) — relevance-aware severity, status, and notification deduplication.
- [Bash timeout policy and failure reporting](bash-timeout-policy-and-failure-reporting.md) — layered deadline audit and typed cross-surface execution outcomes.
- [Background process and terminal lifecycle leaks](background-process-and-terminal-lifecycle-leaks.md) — evidence-first process ownership, diagnostics, reconciliation, and cleanup audit.
- [GitHub Copilot GPT-5.6 API contract](github-copilot-gpt-5-6-api-contract.md) — route exact and suffixed GPT-5.6 offerings through Copilot's Responses API and preserve useful upstream errors.

## Inactive-provider credential expiry produces misleading notifications

**Type:** Bugfix  
**Status:** Observed; not yet assessed  
**Surface:** Provider credential monitoring and TUI notifications

### Observed failure

Provider credentials can expire while an unrelated provider remains active. For example:

- The active session and route use OpenAI/Codex with GPT models.
- Anthropic credentials expire in the background.
- Omegon displays an expired-credentials popup or toast for Anthropic.

The notification is confusing because it interrupts an otherwise healthy session and can imply that the active route has failed or requires immediate operator action.

### Expected behavior

- Credential state remains observable for every configured provider.
- Expiry of an inactive provider does not present as an active-session failure.
- Notifications identify the affected provider and whether it is active, configured as fallback, or currently unused.
- An inactive-provider expiry should use a lower-severity, non-disruptive status surface unless it threatens an imminent route or fallback operation.
- The active provider's expiry or authentication rejection should remain prominent and actionable.
- Repeated background checks must not emit duplicate toasts for the same unchanged credential state.

### Investigation scope

- Identify which credential watcher, provider preflight, or auth-refresh path emits the notification.
- Trace whether notification severity currently has access to the selected route, fallback routes, queued work, and active worker providers.
- Distinguish at least:
  1. active provider,
  2. configured fallback provider,
  3. provider required by queued/delegated work,
  4. configured but currently inactive provider.
- Determine whether expiry is detected proactively from credential metadata or reactively from a provider response.
- Verify that provider switching updates notification relevance immediately.
- Ensure credential details and refresh tokens never appear in logs, events, or UI text.

### Acceptance criteria

1. Expired credentials for an unused provider do not trigger a disruptive popup over a healthy active session.
2. Any retained notification names the affected provider and states that the current route is unaffected.
3. Expiry of the active provider produces a clear, high-priority authentication error with the appropriate login/refresh action.
4. Expiry of a configured fallback warns that failover availability is degraded without claiming the current turn failed.
5. A single credential-state transition produces at most one toast until the state changes again.
6. Switching to a provider with known-expired credentials surfaces the issue before inference dispatch where possible.
7. Provider credential status remains visible through an on-demand status surface even when no toast is emitted.

### Regression scenarios

- Anthropic expires while OpenAI/Codex is active.
- OpenAI/Codex expires while Anthropic is active.
- An expired provider is configured as first fallback.
- An expired provider is referenced by queued or delegated work.
- Credentials refresh successfully after an expiry notification.
- Repeated health polling observes the same expired state.
- The operator switches from a healthy provider to the expired provider.

## Bash tool failures caused by overly aggressive timeouts

**Type:** Bugfix and runtime-policy review  
**Status:** Repeatedly observed; not yet assessed  
**Surface:** `bash` tool execution, process supervision, and tool-result reporting

### Observed failure

A broad set of otherwise valid shell operations fail because Omegon applies timeout limits that are too short for real workloads. Cold builds, dependency resolution, test suites, repository operations, package installation, and commands with temporarily quiet output can be terminated even though they are healthy and making legitimate progress.

These failures are frequently misreported as command failures rather than timeout-policy decisions. They encourage agents to restart expensive work, split commands unnaturally, or bypass the standard Bash tool in favor of interactive terminals.

### Expected behavior

- Ordinary commands receive a practical default execution budget.
- Explicit caller-provided timeouts are honored within a documented safety envelope.
- Long-running but healthy commands are not killed merely because output is temporarily quiet.
- Timeout, cancellation, non-zero exit, spawn failure, and output-idle conditions are distinct typed outcomes.
- A timeout reports elapsed time, configured budget, termination status, and whether descendant cleanup succeeded.
- The agent can select a monitorable terminal session for genuinely long or interactive work without treating that as a workaround for defective Bash defaults.
- Process-tree termination remains bounded and reliable when a real timeout or cancellation occurs.

### Investigation scope

Audit every timeout layer involved in Bash execution:

1. Tool schema/default timeout selection.
2. Event-bus or tool-bus execution deadlines.
3. Bash provider process timeout.
4. Output-idle and heartbeat handling.
5. Harness/API tool-call ceilings outside the child process.
6. Process-group termination and cleanup grace periods.
7. Any caller-specific caps used by validation, delegation, loops, or TUI execution.
8. Serialization or transport timeouts while returning large command results.

Determine whether overlapping deadlines race, whether one layer silently clamps another, and whether timeout units or defaults differ across TUI, ACP, daemon, and delegated execution.

### Policy questions

- Should the Bash default be a larger fixed budget, workload-class-aware, or effectively unbounded with a hard safety ceiling?
- Should output activity extend only an idle deadline while a separate maximum wall-clock deadline remains fixed?
- Which commands should automatically recommend or promote to a background terminal session?
- How should agents discover the effective timeout before dispatch?
- Should validation and known build commands receive policy hints without embedding command-name heuristics into the generic executor?

### Acceptance criteria

1. Cold Rust builds and representative repository test gates complete through `bash` under default or explicitly requested policy without premature termination.
2. A quiet command that remains alive is not mistaken for a dead process solely because stdout or stderr closes or pauses.
3. Caller-specified timeout values are neither ignored nor silently reduced by an inner layer.
4. Timeout results are distinguishable from command exit failures in structured tool output and rendered conversation text.
5. Timeout diagnostics identify which policy layer fired.
6. Cancellation and timeout terminate the full descendant process tree and report cleanup failure if descendants survive.
7. Commands that exceed the true hard ceiling are terminated predictably within a bounded grace period.
8. TUI, ACP, daemon, and delegated Bash execution use the same timeout semantics.
9. Regression tests use real child processes to cover output-active, output-idle, closed-stream, descendant, cancellation, and deadline-race cases.
10. Agents are not encouraged to repeatedly restart a timed-out cold build when no compiler or test failure was observed.

### Regression scenarios

- Cold `cargo`/`just` build exceeding the current default timeout.
- Long test suite with continuous output.
- Long test suite with no output for several minutes.
- Child closes stdout while continuing to run.
- Child closes stderr while continuing to run.
- Parent exits while a descendant remains alive.
- Explicit short timeout intentionally terminates a process tree.
- Operator or agent cancellation races the timeout.
- Large final output is returned near the deadline.
- Identical command dispatched through TUI, ACP, daemon, and delegation surfaces.

## Leaked background processes, terminals, and subprocess trees

**Type:** Bugfix investigation and runtime hardening  
**Status:** Suspected from operational symptoms; leak sources not yet localized  
**Surface:** Background services, interactive terminals, delegated workers, subprocess supervision, and session shutdown

### Problem statement

Omegon appears to leak some background processes or terminal sessions across normal operation, cancellation, failures, restarts, or application exit. The exact producers and lifecycle gaps are not yet known. This item should begin as an evidence-gathering audit rather than assume that every surviving process has the same cause.

Potentially affected facilities include:

- `serve`-managed background services
- Interactive `terminal` sessions
- Bash and validation subprocess trees
- Delegated agents and cleave child workers
- Provider helper processes and local inference servers
- Extension and MCP/plugin processes
- Code-act proxies and sandbox/container helpers
- Browser or authentication workflows
- Daemon, control-plane, and restart handoff processes
- File watchers, schedulers, and recurring-loop workers

### Investigation goals

- Inventory every process-spawning path and assign an explicit owner.
- Record whether each child is session-scoped, turn-scoped, task-scoped, project-scoped, or intentionally persistent.
- Identify the normal completion, cancellation, timeout, error, restart, disconnect, and shutdown cleanup paths for each owner.
- Determine which child processes receive a process group/session, cancellation token, kill-on-drop guard, heartbeat, lease, or persisted runtime record.
- Compare runtime process tables and Omegon's internal registries before and after representative workflows.
- Distinguish true leaks from deliberately persistent services and independently managed external processes.
- Detect stale terminal/session registry entries even when the underlying process has already exited.
- Detect live orphan processes even when the registry entry has disappeared.

### Required observability

Introduce or consolidate a diagnostic projection capable of reporting, without mutation:

- Runtime object ID and facility (`serve`, `terminal`, delegate, extension, and so on)
- Owning session, turn, task, or project
- PID and process-group/session ID where applicable
- Spawn time and last observed heartbeat/activity
- Intended lifetime and persistence policy
- Current registry state and observed OS process state
- Cancellation/termination request time
- Exit status or reason, when known
- Whether descendants remain alive
- Cleanup attempts and their outcomes

Diagnostics must avoid exposing command arguments or environment values that may contain secrets. A redacted executable/label is sufficient.

### Lifecycle invariants

1. Every spawned process has exactly one lifecycle owner or is explicitly marked as externally owned.
2. Every lifecycle owner defines terminal states and cleanup behavior.
3. Cancellation and timeout target the process group or equivalent descendant boundary, not only the immediate PID.
4. Dropping a handle must not silently detach a process unless persistence was explicitly requested.
5. Session exit cleans up session-scoped children before terminal restoration completes.
6. Restart handoff cannot leave both old and new owners supervising the same child.
7. Persisted services have leases or adoption rules that distinguish them from stale orphans.
8. Registry state converges with actual process state after crashes and on next startup.
9. Cleanup is idempotent and safe when the process has already exited.
10. Omegon never uses broad `pkill`/`killall` matching where a tracked PID or process group is available.

### Acceptance criteria

- A process-spawn inventory identifies the owner and lifetime policy for every in-core spawn site.
- Representative create/complete, cancel, timeout, failure, restart, and application-exit tests return process and terminal counts to the expected baseline.
- Terminal sessions that exit naturally are reaped and removed from active listings.
- Cancelling a terminal, Bash command, validation run, delegate, or background service does not leave descendants alive.
- A persisted service survives only when explicitly requested and remains discoverable/adoptable by its registry.
- Stale registry records are detected and pruned or marked stale on startup without killing unrelated processes.
- Shutdown uses a bounded grace period, escalates termination when required, and reports survivors.
- Leak diagnostics can be captured in tests without relying on timing-sensitive manual `ps` inspection.

### Regression scenarios

- Start and stop a `serve` process repeatedly.
- Start a terminal, allow the command to exit naturally, then list sessions.
- Force-stop an interactive terminal with descendants.
- Cancel Bash and validation commands whose children spawn grandchildren.
- Cancel and fail delegated or cleaved workers during startup and execution.
- Restart Omegon while background resources are active.
- Exit normally and via an error path with active session-scoped children.
- Lose the client/TUI connection while daemon-owned work continues.
- Extension or MCP process exits unexpectedly and is restarted or retired.
- Startup encounters stale runtime records whose PIDs have been reused by unrelated processes.
- Two concurrent sessions own separate child trees and one session exits.

### Open questions

- Which facilities are intended to survive an Omegon process exit, and how is that intent represented?
- Should lifecycle ownership converge on one shared process supervisor, or should facilities retain separate registries behind a common projection and shutdown protocol?
- What platform abstraction is required for Unix process groups, Windows job objects, and containerized children?
- Should an operator-facing cleanup command exist, and if so, how can it act only on cryptographically or structurally verified Omegon-owned resources?
