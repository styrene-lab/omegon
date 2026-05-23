+++
id = "reader-workspace-ux-contract"
kind = "document"
title = "Reader Workspace UX Contract"
status = "seed"
tags = ["terminal", "reader", "ux", "commands"]
aliases = ["reader-workspace-ux-contract"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
dependencies = ["managed-reader-workspace", "reader-workspace-zellij-spike"]
open_questions = [
  "Should v1 require `omegon reader session` before `omegon reader open <path>` can create side panes?",
  "Should Omegon offer to install Zellij, print install instructions, or silently fall back?",
  "What should happen if `omegon reader open <path>` is called outside a managed workspace?",
  "Should reader panes persist after Omegon exits, or should they be cleaned up?",
  "How much status should the conversation stream show after opening/replacing a reader pane?"
]
parent = "managed-reader-workspace"
related = ["reader-workspace-zellij-spike", "reader-workspace-substrate-adapter"]
+++

# Reader Workspace UX Contract

## Overview

Define the operator-facing command flow and failure behavior for managed reader panes.

## Candidate flows

### Flow A: installed-substrate required

`omegon reader open <path>` fails with install instructions if Zellij is missing or if the session is not already managed.

Pros:
- Lowest implementation complexity.
- Least surprising.

Cons:
- Weakest product experience.
- Operator still has to understand substrate setup.

### Flow B: explicit managed session

`omegon reader session` starts or attaches a managed Zellij session. Inside that session, `omegon reader open <path>` opens the adjacent Bookokrat pane.

Pros:
- Clear operator intent.
- Avoids surprise re-exec from arbitrary sessions.
- Easier to debug and document.

Cons:
- Requires one explicit setup command.

### Flow C: automatic bootstrap

`omegon reader open <path>` detects missing workspace, starts or attaches a managed workspace, and re-enters Omegon as needed.

Pros:
- Best eventual UX.

Cons:
- Harder to implement safely.
- More surprising if it restructures the current terminal.
- More edge cases around process ownership and session state.

## Initial recommendation

Prefer Flow B for v1:

```text
omegon reader session
omegon reader open <path>
```

This is less magical, easier to test, and still eliminates manual pane orchestration.

## Required failure messages

Define exact behavior for:

- Zellij missing.
- Bookokrat missing.
- Unsupported outer terminal if discovered.
- Not inside managed workspace.
- File missing/unreadable.
- Existing reader pane not found.
- Reader pane failed to spawn.
- Reader pane exited immediately.

## Decisions

### Decision: Explicit managed session is the preferred v1 UX

**Status:** proposed

**Rationale:** Explicit session bootstrap gives the operator control over terminal restructuring and avoids risky automatic re-exec behavior. Automatic bootstrap can be a follow-on once substrate behavior is proven.
