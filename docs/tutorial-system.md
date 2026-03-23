---
id: tutorial-system
title: "Interactive /tutorial system — structured onboarding replacing /demo"
status: exploring
open_questions: []
branches: ["feature/tutorial-system"]
openspec_change: tutorial-system
jj_change_id: rlxnlpoltlwmulkrqqukwzpvmmkwvwuu
---

# Interactive /tutorial system — structured onboarding replacing /demo

## Overview

Evolve /demo from a single guided tour into a proper /tutorial system with discrete lessons, progress tracking, and operator-paced progression. The current /demo has a fundamental problem: the agent treats the entire phase list as one instruction and blasts through without pausing. A tutorial system needs structural enforcement of pacing — the harness controls progression, not the agent's willingness to stop.

## Research

### Current /demo architecture and failure mode

The current /demo:
1. Clones styrene-lab/omegon-demo into /tmp
2. exec's omegon with --initial-prompt-file pointing at demo.md
3. demo.md contains Phase 1 only, tells agent to wait
4. AGENTS.md contains Phases 2-8, agent reads it when told "next"

The failure mode: the agent treats "read AGENTS.md and do the next phase" as "read AGENTS.md and do everything." The STOP instruction is advisory. The agent's context window sees all 8 phases and its completion bias takes over.

Root cause: the harness has no concept of "lesson" or "step." The initial-prompt is just a string. The agent decides when to stop. There's no structural gate between phases — just natural language instructions saying "please stop."

The fix must be structural: the harness feeds one lesson at a time, and the agent literally cannot see the next lesson until the operator advances.

### Architecture proposal: Tutorial as a Feature with harness-controlled pacing



### Game-style tutorial overlay architecture

## The model

Video game first-play tutorials have:
1. A tooltip/callout that appears near the relevant UI element
2. A highlight or pulse on the element being taught
3. Two advancement modes:
   - **Passive:** "Press Enter to continue" (for explanations)
   - **Active:** "Try it now — type /focus" (waits for the operator to DO it)
4. A progress indicator (Step 3 of 8)
5. Skip/dismiss option (Escape or "skip tutorial")

## For Omegon's TUI

```
┌─────────────────────────────────────────────────────┐
│ Conversation area                                    │
│                                                      │
│  ┌──────────────────────────────────┐                │
│  │ 📘 Tutorial (3/8)          [Esc] │                │
│  │                                  │                │
│  │ This is the **instrument panel**. │                │
│  │ The left side shows inference     │                │
│  │ state — context fill, thinking,   │                │
│  │ and memory activity.              │                │
│  │                                  │                │
│  │         ▶ Press Enter ◀          │                │
│  └──────────────────────────────────┘                │
│                                                      │
├──────────────────┬───────────────────────────────────┤
│ Engine Panel     │ ▶▶▶ Instrument Panel ◀◀◀         │
│ model: claude    │  (highlighted/pulsing border)     │
│ tier: Victory    │                                   │
└──────────────────┴───────────────────────────────────┘
```

## Lesson types

1. **Callout** — explain something, Enter to continue
2. **Action** — "Type /focus now" — waits for the operator to actually do it
3. **Watch** — "Watch the tools panel" then the tutorial tells the agent to do something and the operator observes
4. **Interactive** — "Try typing a message to the agent" — waits for any user input

## Step definitions

Each step is a struct:
```rust
struct TutorialStep {
    title: &'static str,
    body: &'static str,
    /// Where to position the overlay relative to
    anchor: TutorialAnchor,  // Conversation, InstrumentPanel, EnginePanel, ToolList, etc.
    /// How the step advances
    trigger: StepTrigger,
    /// Optional: highlight a specific TUI region
    highlight: Option<TutorialAnchor>,
}

enum StepTrigger {
    Enter,                     // press Enter to continue
    Action(String),            // wait for specific slash command
    AgentTurn,                 // wait for an agent response
    AnyInput,                  // wait for any user input
}

enum TutorialAnchor {
    Center,                    // centered overlay
    AboveFooter,               // just above the footer
    NearInstruments,           // near the instrument panel
    NearEngine,                // near the engine panel
}
```

## Key insight: steps are compiled into the binary, not markdown files

This isn't user-authored content. It's Omegon's own onboarding. The steps should be `const` data compiled into the binary — no file I/O, no cloning repos, no parsing markdown. Just a `&[TutorialStep]` array.

The markdown lesson files become unnecessary. The tutorial is a TUI component with hardcoded steps, like a game's tutorial is hardcoded into the game.

## Decisions

### Decision: Individual markdown files per lesson (.omegon/tutorial/01-*.md) with YAML frontmatter

**Status:** decided
**Rationale:** Each lesson is self-contained. The agent sees one file at a time — structurally impossible to read ahead. Files are ordered by numeric prefix. Frontmatter carries title and optional validation criteria. Easy to add, remove, reorder lessons without touching code.

### Decision: Harness-controlled pacing via /next command — agent sees one lesson at a time

**Status:** decided
**Rationale:** The root cause of the demo's pause failure is that pacing was delegated to agent self-control. The fix: the harness injects one lesson as a queued prompt, the agent responds, the operator reads, the operator types /next, the harness injects the next lesson. The agent never sees the lesson list. Structural enforcement, not advisory instructions.

### Decision: Sandbox tutorial project (clone/create temp), not in-place

**Status:** decided
**Rationale:** New operators shouldn't risk their real project during onboarding. /tutorial clones a tutorial repo with pre-seeded content (like /demo does now), exec's omegon inside it. The tutorial repo has lesson files, seed data, and tone directives. Safe to experiment, break things, create/delete files.

### Decision: Progress persists in .omegon/tutorial/progress.json — /tutorial resumes, /tutorial reset starts over

**Status:** decided
**Rationale:** Operators may not finish the tutorial in one sitting. Progress is cheap to store (one JSON file with current_lesson and completed list). /tutorial without args resumes. /tutorial reset clears progress. /tutorial status shows where you are.

### Decision: Natural language advancement — operator types 'next', 'continue', 'ok', 'go' etc. as a normal message, harness intercepts before sending to agent

**Status:** decided
**Rationale:** /next is a UX tax nobody will remember or want to use in a one-time onboarding flow. The tutorial should feel like a conversation. When a tutorial is active, the harness checks incoming user messages against advancement keywords before sending them to the agent. If the message matches, advance the lesson and queue the new prompt instead. The operator just types 'next' or 'ok' naturally. /next still works as an explicit alternative but isn't the primary UX.

### Decision: Tutorial is a TUI overlay widget, not prompt injection — game-style advisor that highlights UI elements and waits for actions

**Status:** decided
**Rationale:** Prompt injection is the wrong abstraction. The tutorial should be a visual overlay in the TUI — a bordered panel that appears on screen, points at specific regions (instrument panel, tool list, engine panel), explains what they are, and advances either on keypress (Enter to continue) or on action (wait for operator to actually type /focus). Like a video game's first-play advisor: tooltip pointing at the health bar → 'this is your health' → press any key → tooltip moves to the minimap. The agent runs normally underneath. The tutorial is a UI layer, not an agent instruction.

## Open Questions

*No open questions.*

## The key insight

The agent must never see more than one lesson at a time. This is a harness constraint, not an agent instruction. The tutorial system is a Feature (like LifecycleFeature) that:

1. Owns a lesson manifest — ordered list of lessons, each with a prompt and optional validation
2. Tracks current position — which lesson the operator is on
3. Controls injection — injects the current lesson as context/prompt, nothing else
4. Gates advancement — /next advances to the next lesson, /prev goes back, /tutorial status shows progress

## Lesson format

Individual markdown files, numbered: `.omegon/tutorial/01-cockpit.md`, `02-tools.md`, etc.

Each file has frontmatter:
```yaml
---
title: "The Cockpit"
order: 1
validates: ["instrument panel visible", "engine panel shows model name"]
---
```

The body is the agent prompt for that lesson — what the agent should explain, demonstrate, or do. The agent sees ONLY this file's content as its instruction.

## Progression mechanism

The Feature registers a `/next` command (or overloads `/tutorial next`). When the operator types `/next`:
1. The current lesson is marked complete
2. The next lesson's content is injected as a user message (like --initial-prompt)
3. The agent responds to the new lesson, and ONLY to it

The agent never sees the full lesson list. It gets one prompt at a time, delivered by the harness.

## Tutorial project

Keep the sandbox approach: /tutorial clones a tutorial repo (or creates a temp project) and exec's omegon inside it, just like /demo. The tutorial repo has:
- `.omegon/tutorial/*.md` — lesson files
- `AGENTS.md` — tone/personality directives (no phase list!)
- Pre-seeded content to work with (source files, memory facts, design nodes)

## Progress persistence

Yes — store progress in `.omegon/tutorial/progress.json`:
```json
{"current_lesson": 3, "completed": [1, 2], "started_at": "2026-03-23T..."}
```

/tutorial resumes where you left off. /tutorial reset starts over.
