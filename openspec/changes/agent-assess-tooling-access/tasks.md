# agent-assess-tooling-access — Tasks

Dependencies:
- Group 1 defines the generic bridge contract and safety model used by later groups.
- Group 2 adapts `/assess` onto shared structured executors after the bridge primitives exist.
- Group 3 onboards lifecycle reconciliation and additional command metadata after assessment results are structured.
- Group 4 covers docs/tests and verifies the v1 allowlist behavior.

## 1. Generic slash-command bridge + command metadata
<!-- specs: harness/slash-commands -->

- [x] 1.1 Add shared bridge primitives in `extensions/lib/slash-command-bridge.ts`
- [x] 1.2 Define normalized result envelope shape: `command`, `args`, `ok`, `summary`, `humanText`, `data`, `effects`, `nextSteps`
- [x] 1.3 Define allowlist metadata for bridged commands, including `agentCallable`, side-effect classification, and confirmation requirements
- [x] 1.4 Refuse commands that are not explicitly allowlisted as agent-callable
- [x] 1.5 Add or extend typings in `extensions/types.d.ts` for structured command execution metadata/contracts
- [x] 1.6 Register a harness-facing tool entrypoint that executes bridged slash commands through the shared metadata/allowlist path
- [x] 1.7 Add tests for allowlisted execution, blocked execution, and confirmation-required responses

## 2. Refactor `/assess` to structured shared executors
<!-- specs: harness/slash-commands -->

- [x] 2.1 Extract `/assess` subcommand logic in `extensions/cleave/index.ts` behind shared executors that return structured results
- [x] 2.2 Preserve existing human-readable `/assess` terminal UX by rendering from structured executor output instead of duplicating logic
- [x] 2.3 Define structured result payloads for `/assess spec`, `/assess diff`, and `/assess cleave`
- [x] 2.4 Include severity summaries, findings, suggested next steps, and lifecycle reconciliation hints in assessment result data
- [x] 2.5 Ensure bridged `/assess` execution does not require parsing TUI-only prose
- [x] 2.6 Add tests covering interactive and bridged execution parity for the v1 assessment commands

## 3. Lifecycle reconciliation integration
<!-- specs: harness/slash-commands -->

- [x] 3.1 Update `extensions/openspec/index.ts` to consume structured assessment outcomes where lifecycle reconciliation is needed
- [x] 3.2 Surface reopened-work / reconciliation signals in machine-readable form for OpenSpec follow-up flows
- [x] 3.3 Update `extensions/design-tree/index.ts` only as needed to consume structured reopen/update signals instead of prose parsing
- [x] 3.4 Confirm the v1 bridged allowlist includes `/assess spec`, `/assess diff`, and `/assess cleave`
- [x] 3.5 Add tests proving an agent can determine pass vs reopen behavior from structured bridge results alone

## 4. Docs, safety, and rollout validation
<!-- specs: harness/slash-commands -->

- [x] 4.1 Expand `docs/agent-assess-tooling-access.md` with bridge architecture, safety model, result envelope, and v1 allowlist
- [x] 4.2 Document side-effect classes such as `read`, `workspace-write`, `git-write`, and `external-side-effect`
- [x] 4.3 Document why arbitrary slash-command execution remains disallowed in v1
- [x] 4.4 Add regression tests ensuring commands are implemented once and rendered twice (structured for agent, text for operator)
- [x] 4.5 Validate the full flow with targeted tests and typecheck, including bridged assessment commands and refusal paths
