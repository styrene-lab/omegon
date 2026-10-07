# Behavior comparison and decision

## Sources

- Current base: `origin/main` at `13760f256662de5cbb8089d38990f7db3c93d89d`.
  Local `main` was older when the worktree was created.
- Read-only reference: separate `/Users/wilson/workspace/styrene-lab/omegon`
  checkout, `feature/tui-presentation-settings` at
  `4aeee3b0e92bdf250af814ef4bdad1957ebf2dd7`.
- Prior integration: `1718e546` (ownership), `97fd7c62` (bounded publication),
  `4ce9c731` (streamed reply retention).
- Existing plans: `tui-dual-presentation` and `tui-project-shell`.

## Bounded parity table

| Behavior | Status at base | Evidence / decision |
| --- | --- | --- |
| Primary-screen inline shell with retained fullscreen buffers | Already integrated | `terminal_buffers.rs`, `terminal_presentation.rs`; reference App/coordinator is superseded by current owners. |
| Canonical completed records, cursor after insertion succeeds | Already integrated | Reference `mod.rs` commits pending publications after `insert_before`; current `inline.rs::publish_inline` settles after insert and flush. |
| Bounded publication and stable streamed prefixes | Implemented beyond reference | `native_publication.rs` budgets, generation/prune reconciliation, known-failure retry and ambiguous-delivery suppression. Keep these current guarantees. |
| Avoid duplicate managed history in inline and borrowed menus | Already integrated | `render.rs` inline early return and borrowed-screen geometry; `draw_inline` renders only the unfinished tail. |
| Quiet startup and inline post-render cosmetics | Already integrated | `run_tui` only starts splash for fullscreen base; `render.rs` bypasses effects for inline and borrowed screens. Editor glow/pulse/completion methods are already no-ops. |
| Inline slash-triggered splash replay | Missing | `slash_commands.rs` unconditionally sets `replay_splash`; `run_tui` selects fullscreen and awaits replay before input/event draining. `startup_splash.rs` polls/reads input concurrently with the input pump. Adopt an inline command guard. |
| Priority cancellation independent of ordinary input congestion | Implemented | `terminal_input.rs` routes Ctrl+C through separate priority ingress; saturation regressions exist. Blocked native-output behavior needs its own fault evidence. |
| Approval owns visible input and restores prior inspector | Implemented | Current interaction owner and existing gated denied-write acceptance restore Project Work selection. No old responder queue import. |
| Completion without AgentEnd and authoritative idle recovery | Implemented | `tests.rs::supervisor_completion_without_agent_end_allows_second_submission`, authoritative-idle and delayed-prior-turn tests. Preserve these tests. |
| Resume from canonical session, restored terminal ownership | Implemented; current-run proof pending | Existing `--menu-backdrop` acceptance resumes the authority session, inspects retained history, and checks primary ownership. No transcript persistence fork. |
| Entire requested operator journey on this exact source | Unverified | Requires current build and private PTY evidence; unit/source inspection is not runtime proof. |

## Decision

Use `base_terminal`, not the transient `inline_active` flag, for the replay policy.
Return the existing short `SlashResult::Display` notice through the normal control
response owner. Do not enter the replay loop, mutate the editor, or change the
current inspector. This is a bounded command policy, not an animation framework.

The existing stress fixture holds a real provider response. Dispatch `/splash`
there before entering Project, then release the stream and verify the normal
second turn, denied write, cancellation, and recovery. Keep absent-AgentEnd and
I/O-failure paths deterministic at their existing unit-test owners.

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
