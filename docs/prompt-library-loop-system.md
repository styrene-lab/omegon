---
id: prompt-library-loop-system
title: "Prompt library and loop execution system"
status: exploring
tags: [tui, prompts, queue, loop, ux]
open_questions:
  - "[assumption] Reusable prompts should not automatically register top-level slash commands; /prompt is the routing surface."
  - "What time system should /loop expose: fixed intervals, cron-like schedules, deadlines, debounce/idle timers, or combinations?"
  - "What event sources should count as /loop triggers or until conditions: test pass/fail, git changes, file changes, task state changes, tool results, operator idle, wall-clock deadline?"
  - "How should prompt invocation arguments be templated and validated without turning prompts into a second skill/config language?"
  - "How should prompt/loop provenance appear in the TUI queue, conversation history, status line, and future workbench?"
  - "What exact anti-prompt-injection checks must run before prompt templates are listed, previewed, queued, looped, or executed?"
  - "How should the prompt library distinguish trusted project/user prompt templates from untrusted prompt text produced by model output, downloaded files, copied snippets, or repository content?"
  - "Should /loop require stronger anti-injection and operator confirmation than /prompt run because it can repeatedly execute injected instructions over time?"
  - "Should CommandSafety be added to omegon_traits::CommandDefinition as concrete fields now, or introduced as an optional metadata object to preserve compatibility with existing extension command definitions?"
dependencies: []
related: []
---

# Prompt library and loop execution system

## Overview

Design a first-class reusable prompt library and loop execution model. Prompts should remain content/templates rather than individual slash commands; /prompt should manage/run/queue prompt invocations, and /loop should schedule repeated prompt invocations with event/time stop conditions without collapsing prompts into skills or extensions.

## Research

### Upstream UX scan — prompt and loop concepts

Live browser result extraction was unavailable in this environment, so this scan is based on known harness patterns rather than fresh citations. Claude Code exposes custom slash commands from markdown prompt files, with project/user scoping and argument interpolation; useful concepts are filesystem-backed prompt definitions and lightweight templating, but the namespace coupling is a cautionary anti-pattern for Omegon. GitHub Copilot/Copilot Chat supports reusable prompt files in repositories (for example `.github/prompts`) and instruction files; useful concepts are repo-local prompt artifacts and IDE discoverability. Cursor/Windsurf lean heavily on rules/workflows/memories rather than arbitrary command proliferation; useful concepts are separating behavioral policy from one-shot prompts. ChatGPT Tasks and automation products expose time-based schedules, reminders, and recurring execution; useful concepts are explicit recurrence, deadlines, and timezone-aware wall-clock triggers. CI/watch tools (`watch`, file watchers, cron, systemd timers, GitHub Actions schedules) offer mature trigger vocabulary: interval, cron, at/deadline, debounce, on-change, max-runs, backoff, and until-success/failure.

### Registry and remote slash path notes

Current code path indicates command registry data originates from EventBus::command_definitions(), is passed into TUI state for the command palette, and can be dispatched through EventBus::dispatch_command(). Remote slash execution in main.rs currently canonicalizes commands and has special handling/allowlists for control and plan commands before rejecting interactive-only commands. Therefore /prompt and /loop should be implemented as native Feature commands and remote slash execution/ACP should either dispatch explicitly-safe registered bus commands or CommandDefinition should grow an availability/safety field. Hardcoding prompt/loop only would satisfy MVP but preserves the architectural split-brain between registry and remote slash surfaces.

## Decisions

### /prompt and /loop are core registered commands across surfaces

**Status:** proposed

**Rationale:** The operator explicitly requires /prompt and /loop placement in the command registry for TUI, CLI remote slash execution, and ACP. Prompt templates remain data addressed by these commands; individual prompt IDs must not become top-level slash commands.

### Prompt IDs do not register as slash commands

**Status:** proposed

**Rationale:** Reusable prompt templates are content/data resolved by /prompt and /loop. Auto-registering prompt IDs would pollute the slash namespace, collide with extensions/skills, and make CLI/ACP command discovery ambiguous.

### CommandDefinition exposes per-surface availability and safety metadata

**Status:** accepted

**Rationale:** The operator decided CommandDefinition needs explicit availability/safety fields for TUI, CLI remote slash execution, and ACP so remote surfaces can expose registered commands without maintaining hidden hardcoded allowlists. This is required for /prompt and /loop to be registry-native across surfaces.

### Prompt and loop surfaces include an anti-prompt-injection gate

**Status:** proposed

**Rationale:** Reusable prompts and loops can turn stored text into repeated agent instructions. The prompt surface must therefore validate provenance, reject or warn on suspicious instruction-overriding content, and preserve a preview/confirmation boundary before execution, especially for /loop.

### Command availability and safety schema

**Status:** proposed

**Rationale:** CommandDefinition should carry explicit per-surface availability and safety metadata. Proposed shape: CommandAvailability { tui: bool, cli: bool, acp: bool } plus CommandSafety { class: LocalOnly | ReadOnly | QueueMutation | StateChanging | ExternalSideEffect | Destructive, requires_confirmation: bool, prompt_injection_sensitive: bool }. Remote slash execution and ACP expose only commands whose availability allows the surface and whose safety class can be handled by that surface's confirmation/permission model.

### Prompt safety verdict schema

**Status:** proposed

**Rationale:** Prompt loading should produce a PromptSafetyVerdict before execution: Clean, Suspicious { reasons }, Blocked { reasons }, or Trusted { trust_record }. Suspicious prompts may be previewed and require explicit operator confirmation before run/queue. Blocked prompts cannot execute until edited or explicitly trusted by a stronger trust flow. Trusted prompts must be bound to path plus content hash so modifications revoke trust.

### Loop scheduling schema

**Status:** proposed

**Rationale:** LoopSpec should schedule PromptInvocation objects, not slash command strings. Proposed primitives: LoopTrigger::Now | Every(Duration) | At(DateTime) | Cron(String) | OnEvent(EventSelector); LoopStopCondition::MaxRuns(u32) | For(Duration) | Until(DateTime) | UntilEvent(EventSelector) | UntilOutcome(Success|Failure|OperatorStop); LoopConcurrencyPolicy::SkipIfRunning | QueueNext | CancelAndReplace. MVP should implement Now, Every, MaxRuns, For, UntilOutcome(OperatorStop), and SkipIfRunning; cron/file-watch/event selectors can be deferred.

## Open Questions

- [assumption] Reusable prompts should not automatically register top-level slash commands; /prompt is the routing surface.
- What time system should /loop expose: fixed intervals, cron-like schedules, deadlines, debounce/idle timers, or combinations?
- What event sources should count as /loop triggers or until conditions: test pass/fail, git changes, file changes, task state changes, tool results, operator idle, wall-clock deadline?
- How should prompt invocation arguments be templated and validated without turning prompts into a second skill/config language?
- How should prompt/loop provenance appear in the TUI queue, conversation history, status line, and future workbench?
- What exact anti-prompt-injection checks must run before prompt templates are listed, previewed, queued, looped, or executed?
- How should the prompt library distinguish trusted project/user prompt templates from untrusted prompt text produced by model output, downloaded files, copied snippets, or repository content?
- Should /loop require stronger anti-injection and operator confirmation than /prompt run because it can repeatedly execute injected instructions over time?
- Should CommandSafety be added to omegon_traits::CommandDefinition as concrete fields now, or introduced as an optional metadata object to preserve compatibility with existing extension command definitions?
