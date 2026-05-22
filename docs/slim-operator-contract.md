+++
id = "5805712d-90e9-4856-8501-b581112a4a16"
kind = "document"
title = "Slim Operator Contract"
status = "draft"
tags = ["tui", "slim", "operator-ux", "contract"]
aliases = ["slim-operator-contract", "om-operator-contract"]
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
open_questions = []
related = [
  "conversation-rendering-engine",
  "tui-visual-system",
  "tool-card-aesthetics",
  "runtime-profile-status-contract",
]
+++

# Slim Operator Contract

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

Slim uses this priority order:

1. conversation prose
2. compact tool evidence
3. pinned active plan
4. composer
5. status/footer hints

Full harness widgets, dashboards, gauges, detailed segment metadata, and operational inventories belong in full mode unless the operator explicitly asks for them.

### Conversation

Assistant prose should render as plain flowing text. It should avoid unnecessary role headers, side borders, and decorative block chrome. Long completed responses may pin to their beginning when that is more useful than leaving the operator at the tail, but the status line must make detached scroll state obvious.

### Tool Evidence

Completed successful tools should collapse to one-line evidence rows by default. The row grammar is:

```text
verb · target · outcome · duration
```

Examples:

```text
read · src/tui/mod.rs · 86 lines
bash · cargo test -p omegon · passed · 42s
set · profile.permissions · persisted
plan · item 2 complete · 3/6
auth · OMEGON_AUTH_JSON_PATH · active
diskutil · list /dev/disk4 · unchanged
```

Rows must include enough information for the operator to understand what happened without expanding the card. Expansion remains available for raw command, arguments, stdout/stderr, structured JSON, and errors.

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
