+++
id = "bba5c619-2b12-4eb9-9cd8-b9ec6d13e5a4"
kind = "design_node"
title = "Junior on-ramp — progressive disclosure aligned with the CVT gearing system"
status = "exploring"
tags = ["ux", "onboarding", "progressive-disclosure", "first-run", "tutorial", "posture", "slim-mode", "effort-tiers", "cvt"]
aliases = ["junior-onramp-progressive-disclosure"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
issue_type = "epic"
open_questions = ["Should experience_level be stored in profile.json or inferred from effort tier choice?", "How aggressively should slim mode suppress harness concepts — silent discard vs. collapsed hints?", "Should the tutorial auto-launch on first session, or remain opt-in with a stronger nudge?", "What is the right threshold for cleave guardrails — file count, diff size, or something else?", "Should /effort auto-downshift trigger after N low-usage turns, or only hint?"]
parent = "null"
priority = "2"
related = ["effort-tiers"]
+++

# Junior on-ramp — progressive disclosure aligned with the CVT gearing system

## Problem

A gung-ho but inexperienced engineer whose mental model is "type prompt, get code" (Claude Code, Hermes, Codex CLI, Cursor chat) hits Omegon and encounters the full power surface from day one. They don't know what they don't need yet.

The result is one of:
- **Combinatorial paralysis** — 4 postures, 7 effort tiers, 5 thinking levels, 4 context classes, presented simultaneously
- **Overkill by default** — they pick Devastator/Omnissiah because it sounds powerful, burn tokens at 10-50x the rate they need for one-file tasks
- **Concept noise** — memory facts, design tree, openspec, cleave appear in responses before the junior has any frame of reference for why they exist
- **Premature exit** — they go back to their previous tool and tell the team "it's too complicated"

This is not a capability problem. Slim mode (`om`) already exists. The problem is that harness concepts leak through slim mode, the posture picker doesn't have a low-knowledge path, and the tutorial is a single step function (off → everything) rather than a gradient.

## Relationship to the effort tier system (CVT)

The effort tier system (`docs/design/effort-tiers.md`) is Omegon's CVT — a single knob (Servitor → Omnissiah) that controls 7 inference decision points in concert: driver model, extraction model, compaction model, cleave child tier, review model, episode generation, and offline driver activation. It already solves the "too many knobs" problem for *inference cost*. This design extends that principle to the *feature surface*.

The key insight: **effort tiers are the spine of progressive disclosure, not just a cost control.** Each tier implies not only a resource envelope but a *complexity surface* — what the operator sees, what tools the agent has, and what concepts appear in responses.

Current state: posture (Explorator → Devastator) sets the three-axis defaults (thinking, context, compaction). Effort tiers (Servitor → Omnissiah) set the 7-point inference config. These two systems overlap on thinking level and model selection but are otherwise independent. The junior encounters both and must understand neither.

Target state: **effort is the single gear lever the operator touches.** Posture becomes an internal implementation detail derived from effort tier. The operator's mental model is one slider, not two enums.

### CVT gear zones — what each zone exposes

The 7 effort tiers map to 3 progressive disclosure zones:

| Zone | Effort tiers | Posture (derived) | Feature surface | Target user |
|------|-------------|-------------------|-----------------|-------------|
| **Cruise** | 1-3 (Servitor → Substantial) | Explorator | Just code. No design tree, no openspec, no cleave in tool list. Memory operates silently. `/help` shows core commands only. | Junior, simple tasks, CI jobs |
| **Drive** | 4-5 (Ruthless → Lethal) | Fabricator | Code + memory + design tree visible. Cleave available but not promoted. `/help` shows standard set. | Intermediate, multi-file work |
| **Overdrive** | 6-7 (Absolute → Omnissiah) | Architect/Devastator | Full surface. Cleave, openspec, lifecycle, delegate all visible. `/help all` equivalent is the default. | Power user, large refactors, orchestration |

This means:
- A junior at effort tier 3 (Substantial) never sees `/cleave` in help, never gets design tree suggestions, never encounters openspec — not because they're disabled, but because they're outside the gear zone.
- `/effort Lethal` (or `/effort 5`) automatically reveals memory and design tree tools — no separate "unlock" step.
- `/effort Omnissiah` is the full harness. Everything visible, everything enabled.
- Operators can always bypass zones: `/help all` shows everything regardless of tier. `/design` works at any tier if typed explicitly. The zone controls what's *promoted*, not what's *possible*.

### CVT interaction with posture

Posture is **no longer a first-class operator control** in the on-ramp flow. Instead:

- Effort tier determines the *derived posture* (see zone table above).
- `/posture` remains available as an advanced override for power users who want fine-grained control (e.g., Architect posture at Ruthless effort — plans carefully but doesn't spend on opus).
- The first-run flow asks for effort preference, not posture.
- `profile.json` stores `effort` as the primary field. `posture` becomes optional override.

This eliminates the "posture vs. effort — which do I set?" confusion. One lever. The CVT.

## Persona

**The junior** — 0-2 years experience. Has used one AI coding tool. Doesn't know git worktrees. Doesn't distinguish between thinking budgets. Wants to type a prompt and get working code. Will not read docs. Will click the first option that sounds good. Will try every slash command out of curiosity. Will judge the tool in the first 15 minutes.

This is not the only persona we serve, but it's the one we lose fastest and convert hardest. Experienced engineers explore at their own pace. Juniors need the tool to meet them where they are and reveal depth as they develop context for it.

## Design

Six workstreams, ordered by implementation dependency. The effort tier system is the backbone — workstreams 1-5 from the previous revision are restructured to build on it.

---

### 1. Truly slim `om` — "just code" mode tied to Cruise zone

**Current state:** `om` launches with `UiSurfaces::lean()` and forces Explorator posture via `bootstrap.rs:53-55`. But the agent's system prompt still includes design tree tools, openspec tools, memory-advanced tools, and lifecycle tools. The LLM can reference these concepts in responses even when the UI doesn't show their panels.

**Target state:** When `om` launches (or `--slim` flag), the agent operates in the **Cruise zone** (effort tiers 1-3):

- Tool groups `memory-advanced`, `lifecycle-advanced`, `delegate` are disabled (already the case via `manage_tools.rs` defaults)
- Additionally disable `design_tree`, `design_tree_update`, `openspec_*`, `cleave_assess`, `cleave_run` tools — these are **removed from the system prompt entirely** so the LLM cannot reference concepts the operator hasn't learned yet
- The `memory_store` tool remains enabled **and visible** — "Stored in Architecture: ..." confirmations appear normally. Memory is ambient intelligence, not a workflow tool; even juniors benefit from seeing the agent learn their project, and the confirmation text is a low-effort discovery moment that pays off across sessions
- `/design`, `/openspec`, `/cleave` commands remain functional if typed explicitly, but are not listed in `/help` output
- `/help` in Cruise zone shows: `/model`, `/effort`, `/think`, `/context`, `/focus`, `/tutorial`, `/ui full`, `/warp`
- The full command set is revealed by `/help all`, by shifting to Drive zone (`/effort Ruthless`), or after `/warp` / `/unshackle`

**The zone boundary is the effort tier, not a separate slim flag.** `om` defaults to Substantial (tier 3, top of Cruise). `omegon` defaults to Ruthless (tier 4, bottom of Drive). The `--slim` flag forces Cruise zone regardless of effort tier. This means the existing `om`/`omegon` binary distinction maps cleanly onto CVT zones without a separate mechanism.

**Implementation notes:**

In the system prompt assembly (wherever the tool list is built for the LLM), read `sharedState.effort.level` (or the Rust equivalent `EffortConfig`) and apply zone-based tool filtering:
- Tiers 1-3 (Cruise): exclude design, openspec, cleave, delegate, lifecycle-advanced tool definitions
- Tiers 4-5 (Drive): exclude delegate, lifecycle-advanced; include design, memory-advanced, cleave
- Tiers 6-7 (Overdrive): include everything

This is more effective than disabling tools in `manage_tools` because it removes them from the LLM's context window — the agent literally cannot reference what it cannot see. The `manage_tools` groups remain as a secondary override for power users who want custom tool sets within a zone.

In `tui/mod.rs`, the help handler (around line 4882) reads the current zone and filters the command list accordingly. Derive zone from effort tier, not from a separate `help_scope` field.

The `memory_store` tool's confirmation message ("stored 3 facts") is controlled in the tool response formatting. In Cruise zone, downgrade this to a status-line indicator (the existing `footer.rs:749-797` memory card) rather than inline text.

**Files touched:**
- `core/crates/omegon/src/bootstrap.rs` — zone-based tool filtering on init
- `core/crates/omegon/src/tui/mod.rs` — zone-aware help filtering
- System prompt assembly (tool list construction) — effort-tier-gated tool exclusion
- `core/crates/omegon/src/features/manage_tools.rs` — new group `harness-lifecycle` containing design + openspec tools, zone-aware defaults

---

### 2. First-run flow — one question, maps to effort tier

**Current state:** `first_run.rs:92-128` presents four postures with thematic names (Explorator, Fabricator, Architect, Devastator). The descriptions are brief but assume the reader understands what "delegates to local models" or "deep reasoning" means. The default recommendation is Fabricator for most detected tool profiles.

**Target state:** Replace the posture picker with an effort-centric experience gate.

```
  How do you want to work?

    [1] Just code — fast, simple, low cost
    [2] Code + memory — remembers your project across sessions
    [3] Full harness — planning, parallel execution, orchestration

  Recommended for you: [1]
```

Mapping to CVT:
- **[1] Just code** → Effort: Substantial (tier 3). Zone: Cruise. Posture: Explorator (derived). Slim UI. Nudge toward `/tutorial`.
- **[2] Code + memory** → Effort: Ruthless (tier 4). Zone: Drive. Posture: Fabricator (derived). Standard UI.
- **[3] Full harness** → Effort: Lethal (tier 5). Zone: Overdrive entrance. Posture: Architect (derived). Full UI.

For the [3] path, offer a follow-up for power users who want to fine-tune:

```
  Fine-tune? (or press Enter to continue)

    /effort Absolute    — opus driver, sonnet extraction
    /effort Omnissiah   — all opus, maximum quality
    /posture Devastator — override to maximum-force posture
```

The experience level (`new` / `intermediate` / `power`) is still stored in `profile.json` for use by the tutorial and alias display systems. It is derived from the choice: [1]→new, [2]→intermediate, [3]→power.

**Why this replaces posture in the first-run flow:**

The junior doesn't care about "Explorator vs. Fabricator." They care about "what will this cost me and what can it do." The effort tier answers both questions with one choice. Posture becomes something power users discover later via `/posture` — an advanced override, not a first-run decision.

The recommendation engine (lines 68-88, mapping detected tools to suggestion) still runs but now maps to effort tiers:
- CLI tools detected (claude-code, codex) → recommend [2] (they're upgrading from a tool that has no memory)
- Ollama detected → recommend [2] (can leverage local inference at lower tiers)
- IDE tools detected (cursor, copilot) → recommend [1] (they have an IDE, want a terminal supplement)
- Fresh install → recommend [1] (safe, cheap, proves value fast)

**Implementation notes:**

In `first_run.rs`, replace `read_posture_choice()` with `read_effort_choice()`. Store `effort` and `experience_level` in `profile.json`. The effort extension reads this on session_start (via `sharedState.effort`, same persistence priority as defined in `effort-tiers.md`: env var > command > config > default).

**Files touched:**
- `core/crates/omegon/src/first_run.rs` — effort-centric flow replacing posture picker
- `core/crates/omegon/src/settings.rs` — `ExperienceLevel` enum, profile field, effort default from profile
- `core/crates/omegon/src/bootstrap.rs` — derive posture from effort tier when no explicit posture override

---

### 3. Plain-English aliases for the CVT and its axes

**Current state:** The control surfaces use thematic names throughout.

| Surface | Current names | Problem |
|---------|--------------|---------|
| Effort tiers | Servitor / Average / Substantial / Ruthless / Lethal / Absolute / Omnissiah | Evocative but opaque — "Ruthless" tells a junior nothing about cost or capability |
| Thinking | Off / Minimal / Low / Medium / High (display: Servitor / Functionary / Adept / Magos / Archmagos) | "Magos" means nothing to a junior |
| Context | Squad / Maniple / Clan / Legion | Military unit sizes are opaque |
| Posture | Explorator / Fabricator / Architect / Devastator | Less opaque but still jargon |

**Target state:** Accept and display plain-English aliases alongside thematic names. The thematic names remain canonical (stored in config, used in logs). The aliases are accepted in all parse paths and shown in UI when `experience_level` is `new` or `intermediate`.

**Effort tier aliases (the primary control):**

| Tier | Thematic | Plain alias | Shown in slim UI | One-line hint |
|------|----------|-------------|------------------|---------------|
| 1 | Servitor | offline | offline | Local models only, zero API cost |
| 2 | Average | local | local | Local models, minimal reasoning |
| 3 | Substantial | standard | standard | Cloud model, light reasoning — daily driver |
| 4 | Ruthless | plus | plus | Cloud model, deeper reasoning |
| 5 | Lethal | pro | pro | Best cloud model for hard problems |
| 6 | Absolute | max | max | All-opus extraction and review |
| 7 | Omnissiah | ultra | ultra | Maximum everything |

This gives operators a natural vocabulary: `/effort standard` for daily work, `/effort pro` when stuck, `/effort offline` on a plane. The thematic names remain for flavor and are shown in full-mode UI.

**Axis aliases (secondary controls):**

| Axis | Thematic | Plain alias | Shown in slim UI |
|------|----------|-------------|------------------|
| Thinking | Off | off | off |
| Thinking | Minimal | fast | fast |
| Thinking | Low | light | light |
| Thinking | Medium | balanced | balanced |
| Thinking | High | deep | deep |
| Context | Squad (128k) | small | small |
| Context | Maniple (272k) | medium | medium |
| Context | Clan (400k) | large | large |
| Context | Legion (1M+) | huge | huge |
| Posture | Explorator | fast | fast |
| Posture | Fabricator | balanced | balanced |
| Posture | Architect | orchestrator | orchestrator |
| Posture | Devastator | maximum | maximum |

**Implementation notes:**

Each enum's `parse()` method already accepts multiple string inputs (`settings.rs` lines 772-781 for ThinkingLevel). Extend these match arms with the alias strings. For effort tiers, extend `parseTierName()` in the effort extension. This is low-risk — it's additive parsing.

For display, add a `plain_name(&self) -> &'static str` method alongside `display_name()`. The status line (`statusline.rs`) and footer (`footer.rs`) check `experience_level` to decide which name to render.

All slash commands accept both vocabularies: `/effort pro` and `/effort Lethal` both set tier 5. `/think deep` and `/think high` both set High. `/context huge` and `/context legion` both set Legion.

**Files touched:**
- `core/crates/omegon/src/settings.rs` — alias parsing + `plain_name()` methods on `ThinkingLevel`, `ContextClass`, `PosturePreset`
- Effort extension `tiers.ts` / Rust equivalent — alias parsing for effort tier names
- `core/crates/omegon/src/tui/statusline.rs` — conditional display name
- `core/crates/omegon/src/tui/footer.rs` — conditional display name
- Slash command handlers in `tui/mod.rs` — alias acceptance (already handled if parse is extended)

---

### 4. Guardrails on overkill — the CVT's "shift-down" advisor

The effort tier system has `/effort cap` to lock a ceiling. But it doesn't tell the operator when they're in the wrong gear. A CVT in a car has a tachometer — the driver can see when the engine is laboring or free-spinning. Omegon needs the equivalent.

Two categories: effort overkill and cleave overkill.

#### 4a. Effort overkill detection (tachometer)

**Problem:** Junior picks "Full harness" in first-run (or manually `/effort Omnissiah`). Every prompt burns opus + high thinking + 1M context. For "add a button to the navbar" this is 10-50x the cost of Substantial.

**Target state:** After the first 3 turns at tier 5+, if no turn has used more than 30% of the context window and no turn has produced reasoning longer than 5k tokens, surface a one-time hint:

```
Tip: your recent tasks fit comfortably in a lower gear.
  /effort standard — faster responses, lower cost
  /effort plus     — balanced for most coding work
  Run /effort to see current settings.
```

This is the CVT's shift-down indicator. It fires once per session, only at tier 5+ (Lethal/Absolute/Omnissiah), only when usage metrics indicate the resource envelope is oversized. It is a hint, not a forced downshift.

**Complementary upshift hint:** If the operator is at tier 3 (Substantial) and the agent hits context limits, retries due to reasoning depth, or produces noticeably truncated output, surface:

```
Tip: this task might benefit from more headroom.
  /effort plus — deeper reasoning, larger context
  /effort pro  — best model for complex problems
```

Together, these form a bi-directional tachometer: too high → suggest downshift, too low → suggest upshift. The CVT metaphor is literal — the system advises the operator toward the right gear for the work.

**Implementation notes:**

Add an `EffortAdvisor` struct (or method on the existing turn-tracking logic) that accumulates:
- `max_context_percent: f32` — highest context usage across turns
- `max_reasoning_tokens: usize` — longest reasoning output
- `turns_observed: u32`
- `retries_from_limits: u32` — count of retries due to context/reasoning limits

After turn 3, evaluate:
- **Downshift:** tier >= 5 && max_context_percent < 0.3 && max_reasoning_tokens < 5000 → emit downshift hint
- **Upshift:** tier <= 3 && (retries_from_limits > 0 || max_context_percent > 0.85) → emit upshift hint

Hints are `HarnessHint` events rendered by the TUI as a dim, dismissable line below the response. The agent does not see these — they're TUI-only, preserving context window.

#### 4b. Cleave overkill guardrail

**Problem:** Junior discovers `/cleave` and tries to parallelize a one-file bug fix across 4 worktrees.

**Target state:** The `cleave_assess` tool (or the `/cleave` slash command handler) performs a pre-flight check before spawning children:

- If the task description references a single file and the diff scope is < 50 lines, surface:
  ```
  This task looks small enough to handle directly — cleave shines on
  multi-file work that can run in parallel. Continue anyway? [y/N]
  ```
- If the plan has only 1 child, surface:
  ```
  Cleave with 1 branch is just... a branch. Want to proceed without
  cleave? [Y/n]
  ```

These are soft gates — the user can override. The goal is to prevent accidental complexity, not to block power users.

**CVT interaction:** At Cruise zone (tiers 1-3), cleave tools aren't in the system prompt, so the agent won't suggest them. This guardrail primarily fires in Drive zone (tiers 4-5) where cleave is available but the operator may not yet understand when it's appropriate.

**Implementation notes:**

The cleave pre-flight logic lives in `core/crates/omegon/src/cleave/`. The assessment step already analyzes the task before spawning. Add scope heuristics to the assessment output:
- Count distinct file paths mentioned in the directive
- Count planned children
- If `files <= 1 && children <= 2`, flag as potentially oversized

The confirmation prompt uses the existing `ControlRequest` confirm flow (the same mechanism as destructive git operations).

**Files touched:**
- `core/crates/omegon/src/cleave/` — pre-flight scope check
- `core/crates/omegon/src/tui/mod.rs` — HarnessHint rendering (new event type, dim + dismissable)
- Agent loop turn tracking — EffortAdvisor accumulation (replaces PostureAdvisor from prior revision)

---

### 5. Progressive tutorial — four lessons that climb the CVT

**Current state:** `tutorial.rs` has three modes (Interactive, ConsentRequired, OrientationOnly) and two paths (Demo project, My project). Both paths cover memory, design, openspec, and auspex in a single linear sequence. A junior who just wants to learn "prompt and code" must sit through the full lifecycle.

**Target state:** Restructure the tutorial into four progressive lessons. Each lesson maps to a CVT zone and ends with a natural stopping point. Completing a lesson *optionally* shifts the operator into the next zone.

| Lesson | CVT zone | Effort tier after | What's introduced |
|--------|----------|-------------------|-------------------|
| 1 | Cruise | Substantial (3) — no change | Prompt, edit, validate. Core coding loop. |
| 2 | Cruise → Drive | Ruthless (4) — offered, not forced | Memory: facts persist across sessions. |
| 3 | Drive | Lethal (5) — offered, not forced | Design tree: structured exploration and decisions. |
| 4 | Overdrive | Absolute (6) — offered, not forced | Cleave + openspec: parallel execution with specs. |

The lesson-to-zone mapping means the tutorial is also a **guided upshift through the CVT**. Each lesson teaches one capability layer, then offers to shift the operator into the gear where that layer is active by default.

#### Lesson 1: Prompt and code (Cruise zone — Hermes parity)

**Goal:** Prove that Omegon does the thing they already know, at least as well.

Steps:
1. Type a prompt → agent reads code, edits a file, runs validation
2. Use `Ctrl+Up` to recall history, `Ctrl+C` to cancel
3. Use `/focus` to read a long response
4. See the status line — context %, turn count, model
5. Try `/effort` to see your current gear

**Ends with:** "That's the core loop — the same thing you'd do in any coding agent, but with a gearbox behind it. Continue to Lesson 2, or start working — type `/tutorial 2` anytime."

**No mention of:** memory, design tree, openspec, cleave.

#### Lesson 2: Memory — the tool remembers (Cruise → Drive transition)

**Goal:** Show that Omegon retains project context across sessions. Introduce the concept that higher gears unlock more features.

Steps:
1. Agent stores a fact about the project (auto or explicit)
2. Show `/memory` to view stored facts
3. Explain that facts persist across sessions — no re-explaining your project
4. Agent uses a previously stored fact in a response
5. Offer: "Memory works best with a bit more reasoning headroom. Shift to plus gear? `/effort plus`"

**Ends with:** "Memory means you don't re-explain your project every session. Continue to Lesson 3, or start working."

#### Lesson 3: Design tree — think before building (Drive zone)

**Goal:** Introduce structured planning for non-trivial work.

Steps:
1. `/design explore "should we refactor X?"` — create an exploration node
2. Agent researches, writes findings
3. `/design decide` — mark a decision
4. Show how decisions persist and inform future work
5. Offer: "For complex problems, pro gear gives the agent deeper reasoning. `/effort pro`"

**Ends with:** "Design nodes capture your reasoning so it survives context windows and sessions. Continue to Lesson 4, or start working."

#### Lesson 4: Parallel execution — cleave and openspec (Overdrive zone)

**Goal:** Show the full power of orchestrated multi-branch work.

Steps:
1. `/openspec` — write a Given/When/Then spec for a change
2. `/cleave` — split the spec into parallel branches
3. Watch children execute in worktrees
4. Review and merge results
5. Show `/effort` dashboard — all 7 tiers, what each controls

**Ends with:** "You've seen the full toolkit. You started in standard gear and shifted up as the work demanded — that's how the CVT works. Use `/effort` to find the right gear for any task. `/help all` to explore everything."

**Implementation notes:**

The `Tutorial` struct in `tutorial.rs` currently holds a single `current: usize` step index into one of three static step arrays. Restructure to:

```rust
pub struct Tutorial {
    lesson: u8,           // 1-4
    step: usize,          // within lesson
    max_lesson_seen: u8,  // for resume
    // ...existing fields
}
```

Each lesson is a separate `&[Step]` array. `/tutorial` with no arg starts at lesson 1 (or resumes). `/tutorial 2` jumps to lesson 2. `/tutorial next` advances to the next lesson.

The lesson-end step uses a new `Trigger::LessonEnd` variant that shows the continue/exit choice and waits for input. If the lesson includes an effort shift offer, the `LessonEnd` trigger also accepts `/effort <tier>` as a valid advance input.

Store `max_lesson_seen` in `profile.json` so that returning users don't repeat completed lessons. The tutorial overlay shows "Lesson 1/4" in its header for orientation.

**Files touched:**
- `core/crates/omegon/src/tui/tutorial.rs` — restructure into 4 lesson arrays, lesson navigation, LessonEnd trigger
- `core/crates/omegon/src/settings.rs` — `max_lesson_seen` in profile
- Tutorial step content — new step text for each lesson

---

### 6. Zone-aware `/effort` dashboard

**Current state:** `/effort` with no args shows current tier name, level, and key settings. The display assumes the operator understands all 7 tiers and their implications.

**Target state:** `/effort` renders a gear-strip visualization that shows the operator where they are and what each zone unlocks:

```
  ── Effort ──────────────────────────────────────────────
  Cruise          Drive           Overdrive
  ╌╌╌╌╌╌╌╌╌╌╌╌╌  ╌╌╌╌╌╌╌╌╌╌╌╌╌  ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
  1   2   [3]     4   5           6   7
  off loc  std    plus pro        max ultra
  ╌╌╌╌╌╌╌╌╌╌╌╌╌  ╌╌╌╌╌╌╌╌╌╌╌╌╌  ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
  prompt+edit     + memory        + cleave
                  + design tree   + openspec
                                  + delegate
  ──────────────────────────────────────────────────────────
  Current: standard (Substantial) · sonnet · light thinking
  /effort plus    shift up
  /effort offline shift down
```

The `[3]` bracket marks current position. Zone labels and feature lists make the progressive disclosure explicit — the operator can see what they gain by shifting up without having to memorize tier names.

In Cruise zone, the visualization is simplified (only shows Cruise detail + "more available →"). In Overdrive, everything is expanded.

**Implementation notes:**

This is a TUI rendering change in the `/effort` slash command handler. The gear-strip is built from the existing `EffortConfig` definitions. Zone boundaries (1-3, 4-5, 6-7) are constants. Feature lists per zone are derived from the tool-filtering logic in workstream 1.

**Files touched:**
- `/effort` command handler in `tui/mod.rs` or effort extension
- Gear-strip render function (new, ~80 lines)

---

## Implementation order

```
  [1] Truly slim om (zone-gated)   — biggest impact, lowest risk, unblocks everything
       ↓
  [2] First-run effort gate        — changes first impression, depends on [1] for
       ↓                             Cruise zone to be a good landing state
  [3] Plain-English aliases        — small, additive, no dependencies
       ↓
  [4] Overkill guardrails          — needs turn metrics wiring, moderate complexity
       ↓
  [5] Progressive tutorial         — largest scope, benefits from [1]-[4] being stable
       ↓
  [6] Zone-aware /effort dashboard — polish, benefits from all above
```

Items [1] and [3] can be implemented in parallel. Item [2] depends on [1]. Items [4] and [5] can be parallelized after [2] lands. Item [6] can land anytime after [3] (needs aliases).

## Non-goals

- **Dumbing down the tool.** Every feature remains accessible. `/help all` shows everything. `/effort ultra` reveals the full surface. This is about sequencing exposure, not removing capability.
- **Auto-detecting experience level from behavior.** Tempting but creepy. Ask once, store the answer, let them change it.
- **Separate binaries or feature flags.** `om` and `omegon` are already the two entry points. We don't need a third. The CVT zones replace the need for more entry points.
- **Rewriting the thematic naming system.** The 40K names are part of the brand. We're adding aliases, not replacing names.
- **Making effort tiers control features at runtime.** The zone-based tool filtering happens at session start and on `/effort` switch. It does not dynamically hide/show UI panels mid-turn. The operator shifts gears deliberately, not automatically.

## Success criteria

- A junior who runs `om` for the first time and picks "Just code" can complete a prompt-edit-validate cycle without encountering any concept they don't understand
- The same junior, one week later, has organically discovered memory (Lesson 2) and shifted to `/effort plus` without being told
- Token cost for a junior's first week is within 2x of what the same tasks would cost in Claude Code / Codex CLI
- The `/effort` dashboard makes the CVT legible — an operator can see where they are, what they have, and what they'd gain by shifting
- No experienced user reports that their workflow was degraded by these changes
- Power users can still set posture independently of effort tier for fine-grained control
