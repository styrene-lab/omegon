---
id: tutorial-demo-project
title: Tutorial demo project — self-seeded repo with live cleave demonstration
status: implemented
parent: tutorial-system
tags: [tutorial, demo, cleave, onboarding, 0.15.0]
open_questions: []
jj_change_id: uumumslvnzlntqvqmvymxykyqqsunlkx
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

### Two operator intents — why a single mode fails both

**Intent A — "Show me Omegon" (demo mode)**
- Operator is new, has no existing project or is in a fresh dir
- Wants to see the full lifecycle: read → design → cleave → verify
- Needs pre-seeded content: design nodes, OpenSpec change, memory facts
- The live cleave is the showpiece — cannot be skipped/degraded silently
- Cost is expected and acceptable if warned upfront
- Best served by: cloning omegon-demo into a temp dir, cd-ing there, running tutorial inside it

**Intent B — "Help me with MY project" (hands-on mode)**
- Operator has an existing codebase they're actively working in
- Wants Omegon to read THEIR code, understand THEIR architecture
- Doesn't want a cloned demo — that's noise in their workspace
- The cleave showpiece is risky/expensive on their real code without preparation
- Best served by: adaptive steps that read the project, bootstrap memory, create first design nodes, introduce OpenSpec without executing a cleave
- This is effectively what /init does but with narration and context

**Why one mode fails:**
- Demo mode in an empty dir: agent tool calls fail, tutorial silently advances (the rc2 bug)
- Hands-on mode for a new user: no cleave, no real showpiece, tutorial feels incomplete
- Adaptive prompts (rc2 fix) paper over the issue but don't resolve the intent mismatch

**The right design:** Two modes, explicit choice at Welcome step. Not auto-detected — operator should consciously choose. Auto-detection is wrong because a fresh dir could be intentional (new project bootstrap) or accidental (forgot to cd).

### Hands-on mode step design

**Act 1 — The Cockpit** (same as demo mode, passive)
Steps 1-4: Welcome (with mode label), Engine, Instruments, Sidebar

**Act 2 — Reading Your Project** (AutoPrompt, adapted)
Step 5: Agent reads source files, stores 3 memory facts about the project
Step 6: Agent uses design_tree list — if nodes exist, explores one; if empty, creates first design node (architecture overview of the project). Operator sees sidebar populate live.

**Act 3 — Bootstrapping Lifecycle** (AutoPrompt, NO cleave)
Step 7: Agent proposes a first OpenSpec change for a real improvement it identified while reading the code. Uses openspec_manage to create proposal + draft spec.
Step 8: Agent reviews the proposal and shows what a cleave plan would look like — explains the lifecycle without executing it (no API cost).

**Act 4 — You're Ready** (same)
Steps 9-10: Power tools, finale with "your project now has design docs, memory, and a first spec"

**Key differences from demo mode:**
- No live cleave execution (Step 7-8 are lifecycle literacy, not execution)
- Steps reference project-specific artifacts discovered in Act 2
- The value delivered is real: the operator's project has memory facts + design nodes after tutorial
- Total cost: ~$0.05-0.10 (2 AutoPrompt turns, no cleave)
- No pre-seeded content required

### Demo mode delivery mechanism options

**Option 1: Clone + exec into temp dir (current mechanism)**
- Clone styrene-lab/omegon-demo to /tmp/omegon-tutorial/
- exec() omegon with the demo dir as cwd
- Tutorial runs inside the demo project — sidebar shows pre-seeded design tree, cleave has its artifacts
- Pro: real project isolation, cleave worktrees work, sidebar populated
- Con: requires git clone (network), exec() restart loses the current session, operator is disoriented by the context switch

**Option 2: Embed demo content inline**
- Ship the demo content as static files baked into the binary (include_str! for key files)
- On /tutorial --demo: create a temp dir, write the files, cd there, re-exec
- Pro: no network dependency, reproducible
- Con: binary size grows, still requires exec() context switch

**Option 3: In-process demo via injected lifecycle context**
- Don't actually change cwd — inject fake lifecycle state into memory
- Tutorial overlay uses a snapshot of demo nodes rather than real docs
- The agent operates on the REAL cwd but sees demo-injected design tree context
- Pro: no exec() required, no network, seamless UX
- Con: agent's tool calls (design_tree, bash, read) hit the real cwd not the demo, so tool results won't match the narrated scenario

**Verdict for 0.15.1: Option 1 is the only clean path for demo mode** — the cleave requires actual worktrees and git history in the demo dir. Option 3 is a hack that breaks at Act 3. Option 2 is the right long-term path but not urgent.

**For 0.15.1 scope:** Hands-on mode is fully in-scope — zero infrastructure needed. Demo mode is in-scope if we can make the clone→exec flow seamless (add a "loading demo project..." interstitial rather than just disappearing).

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

### Decision: Two tutorial modes: /tutorial (hands-on) and /tutorial demo (scripted)

**Status:** decided
**Rationale:** /tutorial with no args runs hands-on mode in the current project — always. This is the right default: operators are in their project, they want to understand Omegon in that context. Acts 2-3 adapt to the project (reads code, bootstraps memory, creates first design node, introduces OpenSpec without executing a cleave). Total cost ~$0.05, no network, no exec() disruption. /tutorial demo runs the scripted demo mode: clones styrene-lab/omegon-demo into /tmp/omegon-tutorial/, exec()s omegon inside it with --no-splash --context-class squad. This is the full showpiece: pre-seeded design tree, live cleave, verification. The Welcome step of the overlay shows which mode is active. No auto-detection — the operator chooses by which command they type. The launch_tutorial_project() function already exists and is already correct — it just needs to be wired up to /tutorial demo.

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
