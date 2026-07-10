+++
id = "5805712d-90e9-4856-8501-b581112a4a16"
kind = "document"
title = "Om Operator Contract"
status = "exploring"
tags = ["tui", "om", "slim", "operator-ux", "contract"]
aliases = ["om-operator-contract", "slim-operator-contract"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = [
  "conversation-rendering-engine",
  "runtime-profile-status-contract",
  "harness-status-contract",
  "tool-surface-matrix",
]
issue_type = "epic"
priority = "1"
open_questions = []
related = [
  "conversation-rendering-engine",
  "tui-visual-system",
  "tool-card-aesthetics",
  "runtime-profile-status-contract",
]
+++

# Om Operator Contract

## Design Thesis

**Om is Omegon with the machinery at rest.** It is the default terminal presentation: conversation-first, outcome-oriented, and deliberately quiet. The name is both the familiar `om` executable identity and a mnemonic for centered attention. It must be reflected in behavior rather than applied as decorative branding.

Om is the first level of a progressive-disclosure ladder:

```text
/ui om       quiet outcomes and essential attention
/ui active   bounded real-time workflow visibility
/ui full     persistent operational evidence and diagnostics
```

`/ui slim` and `/ui lean` remain compatibility aliases for `/ui om`. Mode changes alter projection only. They never discard evidence, change runtime authority, or create a second state path.

### Core law

> Activity mutates in place. Outcomes enter history. Evidence stays attached but collapsed.

This creates three information lifetimes:

1. **Ephemeral activity** — the current operation, useful progress, elapsed time, and bounded partial output. This updates in place and does not accumulate in scrollback.
2. **Durable outcomes** — what changed, what passed or failed, what was published, and what needs attention. This enters the semantic transcript in concise form.
3. **Inspectable evidence** — exact commands, arguments, stdout/stderr, provenance, timestamps, and telemetry. This remains attached to the outcome and can be revealed without having dominated the default view.

## Presentation Ladder

| Contract | `/ui om` | `/ui active` | `/ui full` |
| --- | --- | --- | --- |
| Primary use | ordinary coding and conversation | builds, tests, releases, delegation | debugging and operational control |
| Completed tools | grouped outcomes | grouped outcomes | semantic per-tool rows |
| Current work | one transient line | bounded workflow panel | detailed live cards |
| Workbench | only blocking/decision state | while structured work is active | persistent |
| Telemetry | model, workspace, context or attention | current route and operation progress | provider, tokens, files, phase, provenance |
| Raw args/output | inspect | inspect/bounded tail | detailed and expandable |
| Dashboard/instruments | hidden | hidden | visible |

### Om projection

Om should answer only:

- What is happening now?
- What changed?
- Is operator attention required?

A routine sequence of reads, edits, validation, and git operations should resolve to an episode outcome such as:

```text
✓ Fixed CI path handling · 2 files · 1,842 tests passed
```

While it runs, the same area changes in place:

```text
◌ Running Rust tests…
```

Om must not permanently display turn counters, token I/O, OODA phase, transcript mechanics, files-read totals, version, or workstream ratios. Warnings, permission gates, destructive actions, failures, and stalled operations may expand locally without switching the global mode.

### Active projection

Active is the missing middle: more live structure without log accumulation.

```text
Release 0.27.5                              3/5 · 08:42
├─ changelog and version                    passed
├─ Rust tests                               passed
├─ release build                            running
├─ publish                                  pending
└─ GitHub release                           pending
```

When work completes, the panel collapses into one durable outcome. Workbench state is visible only while active, blocked, awaiting review, or awaiting an operator decision.

### Full projection

Full exposes persistent operational surfaces and semantic evidence. It may show dashboard, instruments, detailed Workbench, provider routing, token/context telemetry, file activity, OODA phase, per-tool durations, provenance, and background terminals.

Full still preserves hierarchy. “All evidence visible” does not mean “every byte inline”; raw payloads remain expandable beneath semantic operation structure.

## Operation Episodes

Compact per-tool rows are not sufficient for Om. They reduce line length but preserve event volume. Om groups related observations into an operator-level episode:

```text
OperationEpisode
├─ intent
├─ state: running | complete | failed | blocked | cancelled
├─ outcome
├─ tool_count
├─ duration
├─ affected_resources
├─ attention_items
└─ evidence[]
```

The first implementation should use authoritative boundaries already present in the harness—agent turn, operator shell command, plan step, delegate run, cleave run, and lifecycle operation—rather than attempting speculative semantic clustering.

The same episode projects differently at each level:

```text
om      ✓ Prepared release 0.27.5 · 9 operations
active  ◌ Preparing release 0.27.5 · 6/9, with current stages
full    all 9 semantic tool observations beneath the episode
```

## 0.28.0 Release Boundary

The Om presentation ladder and the TUI affordances required to support it are part of the 0.28.0 product contract. This is a release target, not a post-release design aspiration.

Repository state currently records a 0.28.0 changelog section dated 2026-07-09 while this contract was accepted afterward. Before shipping, release metadata must be reconciled so the published/tagged 0.28.0 artifact contains these affordances and its exact-version changelog describes them. If an immutable 0.28.0 artifact already exists outside the repository, the operator must choose a new version rather than overwrite it; OCI/package/release tags are not to be mutated in place.

### In scope

- renderer-neutral `Om`, `Active`, `Full`, and `Custom` presentation identity;
- `/ui om`, `/ui active`, `/ui full`, `/ui status`, and three-state cycling;
- `/ui lean` and `/ui slim` compatibility aliases that resolve to Om;
- separation of presentation density from individual surface visibility;
- operation episodes derived from authoritative harness boundaries;
- one mutable activity projection for Om, bounded stage detail for Active, and detailed evidence projection for Full;
- durable outcome rows with attached inspectable evidence;
- attention escalation for permissions, failures, destructive operations, degraded runtime state, and stalls;
- Workbench visibility rules for each presentation level;
- one-line Om status projection with deterministic width shedding;
- semantic transcript/copy/export behavior that omits transient duplication while preserving requested evidence;
- migration of TUI actions, command registry, control/IPC DTOs, settings, tests, help, and release documentation;
- replay and projection tests proving that changing modes does not mutate canonical evidence.

### Explicit non-goals

- changing autonomy, permission, model-routing, or runtime policy when the presentation changes;
- unifying delegate and cleave execution engines;
- introducing speculative LLM-generated episode boundaries;
- redesigning Auspex or the local `/dash` compatibility UI;
- deleting legacy command aliases or stored settings in 0.28.0;
- making Full render unbounded stdout/stderr inline.

## Resolved Design Considerations

### Presentation identity is not a surface bitset

The current `UiSurfaces` bitset conflates dashboard absence with compact rendering. The replacement contract has two layers:

```text
UiPresentationPolicy
├─ level: Om | Active | Full
├─ transcript_density: Outcomes | Operations | Evidence
├─ live_detail: Status | Workflow | Diagnostic
├─ telemetry_density: Essential | Operational | Diagnostic
└─ surfaces: UiSurfaces
```

`Custom` is derived when a user changes individual surfaces or density after applying a named level. It retains `based_on: Om | Active | Full` so renderers have a deterministic density policy. Dashboard visibility must never again be used as a proxy for transcript density.

### Mode changes are pure reprojection

Changing presentation level must not:

- append replacement conversation records;
- delete or recapture tool evidence;
- restart an operation;
- alter tasking, permissions, autonomy, or routing;
- change transcript export truth.

A mode switch invalidates render projections and preserves selection by stable semantic identity. Switching Om → Full must immediately reveal evidence captured before the switch; switching back must restore the same grouped outcome rather than synthesize a new one.

### Episode ownership and boundaries

For 0.28.0, episode boundaries come only from authoritative structure:

| Source | Episode boundary | Stable identity |
| --- | --- | --- |
| Interactive agent work | one operator prompt/agent turn | session id + turn id |
| Operator `!` shell | one `OperatorToolObservation` | observation/segment id |
| Plan execution | one current execution slice or plan item | task/plan revision id |
| Delegate | one delegate operation | `OperationRef` |
| Cleave | one cleave run | `OperationRef`/run id |
| Lifecycle action | one design/OpenSpec/release action | artifact id + revision where available |

Nested delegate/cleave work remains one parent episode with child stages in Active and child evidence in Full. A tool belongs to exactly one episode. If authoritative identity is unavailable, it falls back to a single-tool episode; unrelated tools must never be guessed into one group.

### Episode outcome generation is deterministic

Episode outcomes are projection data, not assistant-authored prose. The reducer computes them from structured evidence using this precedence:

1. blocked permission or required operator decision;
2. failure or cancellation;
3. explicit structured operation result;
4. validation/test result;
5. mutation summary and affected resources;
6. read-only investigation summary;
7. neutral operation-count fallback.

Outcome text must remain bounded, redact secrets using existing adapter policy, and avoid claims not present in evidence. A completed episode may be revised when late authoritative evidence arrives, using the same stable id and a higher revision rather than adding a duplicate row.

### Activity and completion handoff

An episode is live while any authoritative child/tool is running, queued behind an active parent, awaiting a permission decision, or awaiting a required result/merge acknowledgement. Completion is committed atomically from the operator's perspective:

1. final evidence enters canonical state;
2. the durable outcome projection becomes available;
3. the live activity entry is removed.

There must be no frame where successful work disappears from activity before its outcome is projectable, and no persistent frame where the same episode appears as both completed activity and a completed outcome.

### Attention escalation is local and sticky when necessary

Presentation level does not suppress required attention. Om locally expands an attention card for:

- permission and approval gates;
- failed or cancelled work;
- destructive-operation confirmation;
- provider/auth/runtime degradation that changed execution;
- stalled work where elapsed time is actionable.

Permission and destructive gates remain visible until resolved. Failures remain represented by a durable outcome after the live card closes. Informational progress collapses automatically. Local escalation never silently changes the global presentation level.

### Workbench visibility follows lifecycle state

- **Om:** show only pending approval, permission, blocker, unresolved review, or explicit operator decision. A routine active checklist does not permanently occupy the screen.
- **Active:** show active structured work, bounded to the current execution slice and live operation stages.
- **Full:** show persistent structured work and diagnostic metadata.
- **All levels:** terminal states leave the live Workbench. Completion remains available through outcome/history projections.

### Inspection targets semantic identity

`Ctrl+O` and equivalent actions inspect the selected episode first, then its most relevant evidence item. Full mode may navigate individual evidence rows. Inspection state stores stable episode/evidence ids, not screen row indices, and survives reprojection when the target remains available. Escape/collapse returns to the current level without mutating evidence.

### Status and width shedding

Om owns one essential status line. Fields shed from right to left under width pressure:

```text
attention/current activity → model → workspace/branch → context
```

Attention always wins. The line never wraps. Active may add progress and elapsed duration. Full owns diagnostic telemetry. At very small heights, permission/attention, composer, and conversation outrank activity, Workbench, and status in that order.

### Persistence and startup

The selected named presentation level may persist through the existing settings/profile path; transient local expansion and inspection state do not. Migration rules are:

- stored/current `lean` or `slim` → `Om`;
- stored/current `full` → `Full`;
- no setting → `Om`;
- custom surface bitsets → `Custom { based_on: Om, ... }` unless an existing named preset matches exactly.

Presentation settings are client preferences, not session authority. A remote frontend may choose its own projection level while consuming the same canonical state.

### Control and protocol compatibility

Replace boolean runtime-mode requests only with a versioned semantic presentation request that can represent all three levels. During migration, decode legacy `slim=true` as Om and `slim=false` as Full. New emitters use the semantic level. ACP, IPC, WebSocket, and TUI adapters must not infer level from dashboard visibility.

### Transcript and export semantics

Canonical history retains all semantic records. Default Om exports include operator/assistant prose and durable outcomes, without transient activity or duplicate child milestones. Evidence-inclusive export is explicit. Full display density does not silently change `/copy session` or `/transcript`; export policy is selected independently so switching UI modes cannot change archival meaning.

### Accessibility and deterministic testing

State cannot be communicated by color alone: every running, completed, blocked, failed, and cancelled row carries text or a glyph plus text. Motion is bounded to an existing spinner/progress update and may be disabled. Snapshot/replay tests use injected time; elapsed time and stall thresholds must not depend directly on wall-clock timing in fixtures.

## Acceptance Scenarios

### Scenario: Default startup is Om

Given no persisted presentation preference
When the TUI starts
Then the semantic presentation level is Om
And dashboard, instruments, and diagnostic telemetry are hidden
And canonical evidence collection remains enabled.

### Scenario: Active exposes bounded progress

Given an episode with multiple running stages
When the operator selects `/ui active`
Then the current stages and bounded progress are visible
And completed tool evidence does not accumulate as individual transcript rows.

### Scenario: Full reveals prior evidence

Given tools completed while the presentation level was Om
When the operator selects `/ui full`
Then their semantic evidence rows are immediately inspectable
And no new conversation or evidence records are created by the switch.

### Scenario: Return to Om is stable

Given an episode was inspected in Full
When the operator returns to Om
Then the original grouped outcome is shown once
And its stable identity and evidence attachments are unchanged.

### Scenario: Permission overrides quiet presentation

Given Om is active and an operation requires permission
When the permission request enters canonical state
Then a consequence-complete attention card remains visible until resolved
And resolving it does not change the global presentation level.

### Scenario: Completion handoff has no visibility gap

Given a live episode receives its final tool result
When projections advance to the next revision
Then the completed outcome is available before or with removal of live activity
And the episode is not durably duplicated across both surfaces.

### Scenario: Custom surfaces do not redefine density

Given Active is selected
When the operator hides the activity surface
Then the presentation becomes Custom based on Active
And transcript and telemetry density retain Active semantics.

### Scenario: Legacy mode requests remain compatible

Given a legacy control client sends `slim=true`
When the request is decoded
Then Om is selected
And a new client can observe the semantic level without relying on surface booleans.

### Scenario: Export is mode-independent

Given the same canonical conversation and evidence state
When `/transcript` is invoked from Om and from Full
Then the default semantic export content is equivalent
And evidence-inclusive output appears only when explicitly requested.

## Purpose

`om` is Omegon's conventional terminal coding-agent face. It should feel as direct as the mainstream CLI agents operators already know, while remaining a renderer over Omegon's stronger harness state. Slim mode must not create a second control plane, a second permission model, a second plan store, or a second extension/profile path.

The contract is:

- show what the harness already knows
- keep the default terminal experience quiet
- make the next available operator action obvious
- preserve auditability without making the transcript hostile to reading or copying
- route every durable decision through the same underlying profile/session/control state used by full TUI, ACP, daemon, and embedded surfaces

## Operator Questions

At any point, Slim mode should let the operator answer five questions without knowing Omegon internals:

1. What is the agent doing?
2. What is it waiting on?
3. What changed?
4. What can I safely do now?
5. What state will persist?

If a visible Slim element does not help answer at least one of those questions, it should either be removed from Slim, collapsed behind an explicit expansion gesture, or moved to full mode.

## Source Of Truth Rules

Slim is presentation, not policy. It must render existing state rather than inventing new state.

| Operator surface | Source of truth |
| --- | --- |
| Plan progress | IntentDocument recursive tasking state; Slim renders only the current execution-slice projection |
| Tool rows | structured tool call/result segments |
| Permissions | profile permissions, including trusted directories |
| Automation/autonomy | profile-backed automation policy |
| Provider/auth status | provider runtime state and configured auth source |
| Profiles/persona/tone | runtime profile stack |
| Armory installs | unified Armory installer and runtime load paths |
| Transcript/copy | semantic conversation segments |
| ACP/TUI commands | shared control runtime requests |

New Slim UX should fail review if it introduces a shadow store, duplicate command path, separate persistence target, or a display-only state that can disagree with the harness.

## Layout Contract

Om uses this priority order:

1. conversation prose
2. one transient current-activity lane
3. durable operation outcomes
4. blocking or decision-relevant Workbench state
5. composer
6. one essential status line

Active adds bounded workflow detail and active structured work. Full adds persistent harness surfaces, gauges, detailed segment metadata, and operational inventories.

### Conversation

Assistant prose should render as plain flowing text. It should avoid unnecessary role headers, side borders, and decorative block chrome. Long completed responses may pin to their beginning when that is more useful than leaving the operator at the tail, but the status line must make detached scroll state obvious.

### Tool Evidence

In Om, successful tools are inspectable evidence beneath an operation episode, not one durable transcript row per invocation. The visible outcome grammar is:

```text
state · operator intent/outcome · affected resources · useful result
```

Examples:

```text
✓ Fixed CI path handling · 2 files · 1,842 tests passed
✓ Updated profile permissions · persisted to project profile
! Release prepared; GitHub Tests CI failed · inspect
```

Active may reveal bounded child stages while an episode runs. Full may reveal the existing semantic per-tool grammar:

```text
verb · target · outcome · duration
```

Expansion remains available in every mode for raw command, arguments, stdout/stderr, structured JSON, provenance, and errors. No mode may discard evidence that was already captured.

### Pinned Plan

The plan is a pinned operational object, not repeated transcript text. It renders from the current execution-slice projection of IntentDocument recursive tasking and updates in place. Slim must not maintain a second plan store; completed, blocked, suspended, and superseded states are semantic tasking states, not UI-only flags.

The pinned plan should show:

- mode/status
- completed count
- active item
- next item when useful
- blocked, suspended, skipped, complete, failed, or superseded state
- relevant operator actions such as resume, suspend, clear, supersede, or retry

Example:

```text
plan 3/6 · executing
1. done   Fix ...
2. done   Move ...
3. active Store ...
4. next   Assess ...
```

Plan tool calls can remain in the audit trail, but they must not flood the conversation with repeated checklist snapshots.

### Composer And Footer

The footer should expose contextual hints from real session state. It should not become a permanent command cheat sheet.

Examples:

```text
End tail · /copy latest · /transcript
/plan advance · /plan skip · /plan suspend · automation: guarded
plan blocked · /plan resume · /plan supersede
plan complete · /plan clear
view detached ↑42 · End tail
permission pending · y once · a always · n deny
```

Hints are allowed to rotate or shed at narrow widths, but they must never wrap the status line.

## Permission Contract

Permission prompts must be consequence-complete. The operator should never need to know a hidden key in advance.

Every permission prompt should show:

- tool or operation
- target path/resource
- reason for the gate
- persistence target for durable grants
- exact key map
- consequence of "once" versus "always"

Canonical shape:

```text
Permission required
Tool: read
Target: /path/to/file
Reason: grant required for this operation
Persist: project profile permissions

[y] once   [a] always + save   [n/Esc] deny
```

ACP, TUI, and future host-panel prompts may differ visually, but they must call the same permission grant path and persist to the same profile state.

## Automation Contract

Automation is an operator-visible mode, not a hidden retry behavior. Slim should display the active automation policy when it affects what the agent will do next.

Modes:

- `ask`: stop unless the operator explicitly asks to proceed
- `guarded`: continue through low-risk next steps, stop at meaningful gates
- `flow`: continue through expected plan/tool progress, stop at hard boundaries
- `autonomous`: continue until completion, exhaustion, or hard boundary

Hard boundaries are never bypassed by automation:

- permission gates
- security gates
- plan approval gates
- explicit interrupts
- max-turn and timeout budgets
- provider/auth failures
- destructive operation gates

The goal is to eliminate "I will do X next" stalls when the operator has already authorized action, not to reintroduce constant "would you like me to proceed?" prompts.

## Copy And Transcript Contract

Slim must be pleasant to copy from. The primary export path is semantic transcript data, not terminal scrollback scraping.

Required surfaces:

- `/copy latest`: latest assistant response
- `/copy latest plain`: latest assistant response without markdown
- `/copy session`: semantic session transcript
- `/transcript`: clickable Markdown file export
- `/transcript scrollback`: explicit terminal scrollback dump

Transcript exports should not include duplicated pinned plan snapshots, repeated status panels, or full expanded tool payloads unless the operator requested that form.

## Command Discovery Contract

`/help` in Slim should show common daily controls first:

- prompt/edit/validate flow
- permissions
- automation
- plan
- copy/transcript
- profile/auth/model
- mode switch to full harness

`/help all` may reveal full harness controls. Slim should avoid promoting OpenSpec, design tree, cleave, daemon, and dashboard concepts unless the operator asks for the full surface.

## ACP/TUI Parity Contract

For each durable operator operation, ACP and TUI must share the conceptual operation and persistence path:

- permissions
- automation
- plan state
- profile view/capture/apply/edit
- Armory/skill/extension installs
- auth status
- transcript/session export where applicable

Different clients may render different controls, but they must not introduce incompatible state names, persistence locations, or lifecycle semantics.

## 0.28.0 Implementation Plan

The implementation order is deliberately substrate-first. Rendering grouped rows before stable identity and reducers exist would create TUI-only state and make mode switching lossy.

### Phase 0 — Release and lifecycle reconciliation

**Goal:** make the release target truthful before mutation work expands.

- Bind this design node to the 0.28.0 release workstream and mark it `implementing` when code work starts.
- Reconcile whether a 0.28.0 tag/artifact exists remotely; do not overwrite immutable release artifacts.
- Create or update the lifecycle task artifact from the phases below, with scenario-owned acceptance tests.
- Keep `[Unreleased]` and the exact 0.28.0 changelog section synchronized with the eventual shipped behavior.

**Exit:** release ownership and version semantics are explicit; implementation tasks and acceptance scenarios agree.

### Phase 1 — Semantic presentation policy

**Primary files:**

- `core/crates/omegon/src/surfaces/layout.rs`
- `core/crates/omegon/src/ui_runtime/actions.rs`
- `core/crates/omegon/src/control_runtime.rs`
- IPC/WebSocket action DTO adapters that currently carry `SetRuntimeMode { slim }`

**Work:**

1. Introduce `UiPresentationLevel::{Om, Active, Full}` and a policy carrying transcript, live-detail, telemetry, and surface settings.
2. Preserve `UiSurfaces` as subordinate visibility configuration; remove `is_compact() == !dashboard` as a policy decision.
3. Define `Custom { based_on, surfaces }` semantics or an equivalent normalized representation.
4. Replace new boolean mode requests with semantic level requests; retain legacy decode compatibility.
5. Add serialization/migration tests and pure policy matrix tests.

**Exit:** every adapter can name all three levels without consulting Ratatui or inferring from dashboard visibility.

### Phase 2 — Commands, settings, and interaction affordances

**Primary files:**

- `core/crates/omegon/src/tui/mod.rs`
- command registry/help definitions
- settings/profile persistence modules
- shared menu projections

**Work:**

1. Add `/ui om`, `/ui active`, `/ui full`, `/ui status`, and `/ui next`.
2. Map `/ui lean` and `/ui slim` to Om with compatibility messaging that does not pollute transcript history.
3. Change `Ctrl+G` to cycle Om → Active → Full → Om.
4. Update the UI menu to expose the three-level ladder and show `custom · based on <level>` when appropriate.
5. Persist named level through the existing preference path; migrate legacy values.
6. Update `/help`, empty-composer hints, and command completion.

**Tests:** slash routing, menu actions, keyboard cycle, migration, custom-base retention, and remote action parity.

**Exit:** the level is operator-selectable and survives restart without changing runtime authority.

### Phase 3 — Operation episode substrate

**Primary files:**

- new renderer-neutral episode module under `core/crates/omegon/src/surfaces/`
- `surfaces/conversation.rs`
- `surfaces/operations.rs`
- conversation/session projection adapters

**Work:**

1. Define stable episode/evidence references, state, revision, deterministic outcome fields, affected resources, attention items, and child-stage summaries.
2. Build a pure reducer over canonical conversation/tool/operation records.
3. Implement authoritative grouping for turn, operator shell, plan step, delegate, cleave, and lifecycle boundaries.
4. Use single-tool fallback when no boundary identity exists.
5. Add deterministic summary precedence, redaction hooks, bounded resource lists, and late-evidence revision behavior.
6. Preserve a direct mapping from every episode to its evidence records.

**Tests:** table-driven reducer fixtures for read-only turns, edits plus validation, failures, cancellation, permission blockage, operator shell, nested delegate/cleave, late completion evidence, and fallback grouping.

**Exit:** Om outcomes and Full evidence are two projections over the same stable episode state.

### Phase 4 — Activity/completion handoff

**Primary files:**

- `core/crates/omegon/src/surfaces/activity.rs`
- operation progress adapters
- event/reducer path in `tui/mod.rs`
- replay fixture support

**Work:**

1. Project one primary live episode for Om, with attention taking priority over routine activity.
2. Project bounded stage trees for Active and detailed live entries for Full.
3. Define atomic revision ordering for final evidence → durable outcome → activity removal.
4. Preserve running, queued, blocked, cancelled, and stalled semantics.
5. Inject time into projection tests and define a conservative stall threshold/configuration source.

**Tests:** no visibility gap, no durable duplication, permission stickiness, cancellation, concurrent tool priority, and delayed child completion.

**Exit:** live rows mutate in place and reliably collapse into one outcome.

### Phase 5 — Conversation and evidence projection

**Primary files:**

- `core/crates/omegon/src/tui/conversation.rs`
- `core/crates/omegon/src/tui/segments.rs`
- `core/crates/omegon/src/tui/segment_components/`
- `conversation_render_projection.rs`

**Work:**

1. Add an outcome/episode segment component keyed by stable episode id.
2. Make Om hide routine per-tool rows in favor of episode outcomes without deleting source segments.
3. Make Active retain grouped completed history while exposing current stage structure outside scrollback.
4. Make Full render semantic evidence rows nested/associated with episodes.
5. Move `Ctrl+O` inspection from “latest expandable tool” to episode-first semantic selection, with evidence fallback.
6. Ensure switching levels preserves scroll anchor and selection by stable identity.

**Tests:** semantic projection snapshots at all three levels, mode-switch round trips, narrow widths, failures, selection stability, and pre-switch evidence reveal.

**Exit:** the transcript density matches the contract at every level.

### Phase 6 — Layout, Workbench, and essential status

**Primary files:**

- `core/crates/omegon/src/tui/layout_projection.rs`
- `core/crates/omegon/src/tui/workbench.rs`
- `core/crates/omegon/src/tui/statusline.rs`
- footer/instrument projection adapters

**Work:**

1. Replace `is_slim` layout branching with explicit level/policy inputs.
2. Give Om a single-line activity lane, decision-only Workbench, composer, and one non-wrapping essential status line.
3. Give Active a bounded workflow panel and active Workbench with terminal-height budgets.
4. Retain Full dashboard/instruments/diagnostic status behavior.
5. Implement deterministic width/height shedding and attention priority.
6. Reuse the existing structured live-operation rows as Active/Full stage detail rather than discarding that work.

**Tests:** layout matrices across levels, 40/80/120-column widths, constrained heights, permission precedence, empty activity, active plan, delegate/cleave progress, and custom surfaces.

**Exit:** Om is visually calm, Active remains operationally useful, and Full retains observability.

### Phase 7 — Transcript, copy, ACP, and replay parity

**Primary files:**

- transcript/copy exporters
- `core/crates/omegon/src/acp/surfaces.rs`
- IPC/web semantic projection adapters
- `ui_runtime/replay` fixtures

**Work:**

1. Define default outcome-oriented export independently from presentation level.
2. Add explicit evidence-inclusive export where an equivalent option does not already exist.
3. Exclude transient activity and duplicate Workbench snapshots from semantic transcript output.
4. Expose presentation capability and episode/outcome DTOs to clients without requiring identical visual layouts.
5. Add replay fixtures proving identical canonical state produces Om, Active, and Full projections and that actions do not mutate evidence.

**Exit:** alternate frontends can reproduce the semantics, and transcript meaning is mode-independent.

### Phase 8 — Hardening and release gates

**Work:**

1. Run focused crate tests after every phase.
2. Run `cargo test -p omegon --locked`, `just lint`, `just test-rust`, and `just link` before the release commit.
3. Review security/redaction behavior for outcome summaries and exported evidence.
4. Exercise a manual scenario matrix: routine edit/test, failed test, permission prompt, operator `!` shell, long terminal build, delegate, cleave, plan completion, provider degradation, and mode cycling mid-operation.
5. Verify no stale `lean ↔ full`, `is_slim`, or boolean runtime-mode assumptions remain in active product paths except documented compatibility decoding.
6. Update exact 0.28.0 release notes and operator documentation, then tag/publish only from the reconciled release line.

**Release acceptance:** all contract scenarios pass, all repository gates pass, Om is the default, and Full can reveal evidence captured entirely while Om was active.

## Review Checklist

Use this checklist for Slim UX changes:

- Does the change answer one of the five operator questions?
- Does it render existing harness state instead of creating shadow state?
- Is the action, target, outcome, and persistence consequence visible?
- Does it preserve clean copy/paste behavior?
- Does it avoid flooding scrollback with state that belongs in pinned UI?
- Does it keep ACP/TUI behavior conceptually unified?
- Does it remain useful at narrow terminal widths?
- Does it keep full harness concepts out of the default `om` path unless explicitly requested?
