---
id: tutorial-demo-project
title: Tutorial demo project — self-seeded repo with live cleave demonstration
status: implementing
parent: tutorial-system
tags: [tutorial, demo, cleave, onboarding, 0.15.0]
open_questions: []
jj_change_id: pqvwovtvltpkrkxrmwtynquqtnvnotkn
issue_type: feature
priority: 2
---

# Tutorial demo project — self-seeded repo with live cleave demonstration

## Overview

Rework the tutorial's cloned project to be a self-seeded demonstration environment. The current 'type a message for tool use' step is weak. Instead: the tutorial repo should be pre-seeded with design nodes, OpenSpec changes, and a prepared cleave plan so the operator watches a real 5-branch cleave run (2×3 topology) execute live. Design nodes update in the sidebar as implementation progresses. The operator experiences the full lifecycle — design → spec → decompose → implement → verify — as a guided walkthrough, not an abstract explanation.

## Research

### Current tutorial architecture

**Two tutorial systems exist:**

1. **Built-in overlay** (`tui/tutorial.rs`) — 7 compiled steps, game-style tooltip overlay that highlights UI elements. Triggered by `/tutorial` when no lesson files exist. Steps: Welcome → Engine → Inference → Tools → Slash Commands → Focus → Ready. Advances via Tab (passive) or action (command/input triggers).

2. **Lesson-file system** (`TutorialState`) — reads `.omegon/tutorial/01-*.md` files with YAML frontmatter. Each lesson's content is injected as a prompt. Advancement via `/next` or natural language ('next', 'ok'). Falls back to this when `.omegon/tutorial/` dir exists.

**Demo repo:** `styrene-lab/omegon-demo` cloned to `/tmp/omegon-tutorial/`. Contains: a tiny Rust crate (`greet()`), AGENTS.md, lesson files, pre-seeded memory facts, 2 design docs. Launched via `launch_tutorial_project()` which exec's omegon inside the cloned dir.

**Current test-project** (`test-project/` in-repo): Same structure. Cargo.toml + lib.rs + main.rs + 8 lesson files + 2 design docs.

**What's weak:**
- Step 4 "Tool Activity" just says "ask a question and watch" — no seeded scenario
- No lifecycle demonstration (design → spec → cleave → verify)
- No sidebar/dashboard interaction
- The lesson-file system and overlay system are disconnected — operator sees either one or the other, never a coordinated experience

### Tutorial flow design — the operator journey

The tutorial should tell a story with the demo project as the stage. The operator doesn't just learn buttons — they watch Omegon do real work on a real (small) project, with the overlay narrating what's happening and why.

**Proposed flow:**

**Act 1 — The Cockpit** (overlay steps, no agent interaction)
1. Welcome — what Omegon is, what you're about to see
2. Engine Panel — model, tier, thinking, context
3. Instruments — context bar, memory strings, tool recency
4. Sidebar — design tree, node statuses, navigation (Ctrl+D)

**Act 2 — The Agent Works** (agent does real tasks, overlay narrates)
5. 'Watch this' — overlay tells operator to watch, then the tutorial auto-sends a prompt asking the agent to read the project and store memory facts. Operator sees tools light up (bash, read, memory_store), instruments respond, sidebar updates.
6. Design exploration — overlay explains design nodes, then auto-sends a prompt asking the agent to explore a pre-seeded design question. The agent reads the doc, adds research, makes a decision. Operator sees the sidebar node status change.

**Act 3 — The Lifecycle** (the showpiece — live cleave)
7. Decomposition — overlay explains cleave. The tutorial auto-triggers a pre-prepared `/cleave` on a pre-seeded OpenSpec change with 3 branches. Operator watches the cleave progress in real time — child branches appear, tools fire, branches merge.
8. Verification — after cleave completes, overlay explains `/assess spec`. The tutorial auto-triggers assessment. Operator sees pass/fail.

**Act 4 — You're Ready**
9. Focus mode toggle, calibrate mention, key bindings summary
10. Final — "you're ready, /help for everything, /tutorial to replay"

This is ~10 steps, up from 7. The key difference: Acts 2-3 involve the agent doing real work while the overlay narrates. The overlay needs a new trigger type: `AutoPrompt(String)` — sends a prompt automatically and waits for the agent to finish before advancing.

## Decisions

### Decision: Tutorial project is a small Rust CLI tool with pre-seeded lifecycle artifacts

**Status:** decided
**Rationale:** The project needs to be: (1) small enough that cleave branches finish in <60s, (2) interesting enough that the work is visible and inspectable, (3) Rust because that's Omegon's own language and the operator likely uses it. A CLI tool with 3-4 modules (config parser, formatter, validator, CLI interface) gives enough surface for a 3-branch cleave. Pre-seeded: 2-3 design docs (one decided, one exploring), 1 OpenSpec change with specs and tasks ready for cleave, ~10 memory facts giving the project a 'lived-in' feel, a .omegon/milestones.json with a '0.2.0' milestone.

### Decision: Tutorial cleave uses retribution tier with a cost warning upfront

**Status:** decided
**Rationale:** A 3-branch cleave on gloriana could cost $2-5 in tokens — unacceptable for an onboarding flow the operator might run multiple times. Retribution tier is the cheapest cloud option and still demonstrates the full cleave lifecycle. The tutorial overlay shows an upfront notice: 'This demo will use ~$0.30 of API credits for the cleave demonstration. Press Tab to continue or Esc to skip.' If local inference is available, prefer that. The --context-class squad flag already constrains context size.

### Decision: Overlay steps reworked into 4 acts (10 steps) with new AutoPrompt trigger

**Status:** decided
**Rationale:** The current 7 steps are all passive (look at this, press Tab). The new flow has 4 acts: Cockpit (passive UI tour), Agent Works (auto-prompted tasks the operator watches), Lifecycle (live cleave), Ready (wrap-up). This requires a new trigger type — AutoPrompt — that sends a prompt to the agent automatically and waits for the agent turn to complete before the overlay advances. The overlay narrates what's happening while the agent works. ~10 steps total. The overlay and lesson-file systems merge: overlay provides the visual narration, auto-prompts provide the agent instructions.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `core/crates/omegon/src/tui/tutorial.rs` (modified) — Add AutoPrompt trigger type, rework STEPS to 10-step 4-act flow, add cost warning step
- `core/crates/omegon/src/tui/mod.rs` (modified) — Wire AutoPrompt trigger — send prompt on step enter, advance on agent turn complete
- `test-project/` (modified) — Rework tutorial project: Rust CLI tool with pre-seeded design docs, OpenSpec change, memory facts, milestone
- `test-project/openspec/changes/add-validation/` (new) — New: pre-seeded OpenSpec change ready for cleave (proposal + specs + tasks)
- `test-project/docs/` (modified) — Pre-seeded design docs: one decided (cleave-ready), one exploring

### Constraints

- AutoPrompt must not fire if the operator presses Esc (skip tutorial)
- Cost warning step must block before any API-consuming action
- Cleave branches must complete in <60s on retribution tier
- Tutorial must work without local inference (cloud-only operators)
- The demo repo must be a valid git repo (cleave needs worktrees)
