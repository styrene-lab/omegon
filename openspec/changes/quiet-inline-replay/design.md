# Behavior comparison and decision

## Post-merge disposition — 2026-10-07

PR #249 merged the inline replay guard at
`77996814af7720a45f15fd482ac8579093ebdd6f`. The implementation is accepted, and
the seven scoped tasks remain complete. PR-head CI and earlier frozen-binary
acceptance passed. Exact `77996814` headless stress/resume acceptance also passed.
Post-merge CI had 29 successful jobs and one Rust-build timeout; it is not all
green. [Verification](verification.md) identifies each evidence boundary.
Native administrative closure remains blocked, as recorded in the
[lifecycle concern](../../../docs/lifecycle-closure-reconciliation.md).

## Historical comparison sources — 2026-10-07, before implementation

- Original base: `origin/main` at `13760f256662de5cbb8089d38990f7db3c93d89d`.
  Local `main` was older when the worktree was created.
- Read-only reference: separate reference checkout, `feature/tui-presentation-settings` at
  `4aeee3b0e92bdf250af814ef4bdad1957ebf2dd7`.
- Prior integration: `1718e546` (ownership), `97fd7c62` (bounded publication),
  `4ce9c731` (streamed reply retention).
- Existing plans: `tui-dual-presentation` and `tui-project-shell`.

## Bounded parity — disposition at merged main `77996814`

Historical status refers to `13760f25`. Verification distinguishes the earlier
source-bound frozen artifact from the later pristine combined-main build.

| Behavior | Historical status at base | Landed disposition and evidence |
| --- | --- | --- |
| Primary-screen inline shell with retained fullscreen buffers | Already integrated | `terminal_buffers.rs`, `terminal_presentation.rs`; reference App/coordinator is superseded by current owners. |
| Canonical completed records, cursor after insertion succeeds | Already integrated | Reference `mod.rs` commits pending publications after `insert_before`; current `inline.rs::publish_inline` settles after insert and flush. |
| Bounded publication and stable streamed prefixes | Implemented beyond reference | `native_publication.rs` budgets, generation/prune reconciliation, known-failure retry and ambiguous-delivery suppression. Keep these current guarantees. |
| Avoid duplicate managed history in inline and borrowed menus | Already integrated | `render.rs` inline early return and borrowed-screen geometry; `draw_inline` renders only the unfinished tail. |
| Quiet startup and inline post-render cosmetics | Already integrated | `run_tui` only starts splash for fullscreen base; `render.rs` bypasses effects for inline and borrowed screens. Editor glow/pulse/completion methods are already no-ops. |
| Inline slash-triggered splash replay | Missing before implementation | Implemented: `slash_commands.rs` checks the inline session base and returns the notice before setting `replay_splash`. Inline and borrowed-fullscreen regression cases passed; the frozen-artifact stress capture retains notice and cancellation visibility. |
| Priority cancellation independent of ordinary input congestion | Implemented | `terminal_input.rs` routes Ctrl+C through separate priority ingress; saturation regressions exist. Blocked native-output behavior needs its own fault evidence. |
| Approval owns visible input and restores prior inspector | Implemented | Current interaction owner and existing gated denied-write acceptance restore Project Work selection. No old responder queue import. |
| Completion without AgentEnd and authoritative idle recovery | Implemented | `tests.rs::supervisor_completion_without_agent_end_allows_second_submission`, authoritative-idle and delayed-prior-turn tests. Preserve these tests. |
| Resume from canonical session, restored terminal ownership | Implemented; proof was pending | Passed on the earlier frozen artifact and exact combined-main `77996814` with `--menu-backdrop`: canonical history, preserved drafts, inspection, primary-screen restoration, and cleanup. |
| Scoped inline operator journey | Unverified before acceptance | Earlier frozen-artifact and exact `77996814` stress/resume passed. PR-head CI passed at `f21cc8d6`; exact-main CI remains blocked by the Rust-build timeout. |

## Implemented decision

Use `base_terminal`, not the transient `inline_active` flag, for the replay policy.
Return the existing short `SlashResult::Display` notice through the normal control
response owner. Do not enter the replay loop, mutate the editor, or change the
current inspector. This is a bounded command policy, not an animation framework.

The stress fixture holds a local provider response and dispatches `/splash`
before entering Project. It then releases the stream and verifies the normal
second turn, denied write, cancellation, and recovery. Absent-AgentEnd and
I/O-failure paths remain deterministic at their existing unit-test owners.

## Acceptance discoveries

Native insertion and redraw are separate terminal writes. A second capture after
a successful readiness predicate can observe a temporarily cleared composer.
The replay checkpoint now retains the same observed frame used by its predicate.
Both the notice and cancel control must be present in that frame.

The existing fixture's generic dummy OpenAI key enabled external model discovery.
Removing it made the current startup credential gate report disconnected despite
the configured project endpoint. Keep that compatibility credential and route
external HTTP(S) through a rejecting loopback proxy. This changes only the private
child environment, not production credential or provider policy.

## Architecture impact — assessed at `77996814`

The assessment revision is `77996814af7720a45f15fd482ac8579093ebdd6f`.
The [LikeC4 model](../../../site/diagrams/likec4/context.c4) remains an observed
grouping. Stable references below identify its existing `hostComponents`
elements and relationships.

- **Native inline flow:** `omegon.host.interfaces` receives operator commands;
  `omegon.host.interfaces → omegon.host.orchestration` submits and cancels work
  and observes progress. The
  [inline slash guard](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/tui/slash_commands.rs#L1232-L1243)
  is interface-local policy. It keeps input and event processing with the existing
  TUI owners. [Completion regressions](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/tui/tests.rs#L2765)
  preserve authoritative completion and ordinary second submission.
- **Tool-bearing request preparation:**
  `omegon.host.orchestration → omegon.host.context` assembles turn context, and
  `omegon.host.orchestration → omegon.host.inference` requests streamed responses.
  The [shared envelope](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/model_request.rs)
  validates prepared inputs across existing adapters. Invocation permission
  remains with `omegon.host.tools`; preparation does not grant execution authority.
- **Provider transport repair:** existing `omegon.host.inference → inferenceProvider`
  still owns model inference. The
  [provider adapters](https://github.com/styrene-lab/omegon/blob/77996814af7720a45f15fd482ac8579093ebdd6f/core/crates/omegon/src/providers.rs)
  retain that boundary for the Ollama Cloud request repair.

These changes refine behavior inside existing owners. They require no new
component or topology change. The model's generic operator-interface
relationship remains appropriate. This is a source and responsibility assessment,
not a claim of rendered-model verification after `77996814`.
