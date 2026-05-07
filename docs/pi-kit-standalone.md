+++
id = "c425d5a7-47a2-4a9c-991b-ddc83da056f4"
kind = "document"
title = "Make Omegon Standalone — Subsume the pi Harness"
status = "archived"
tags = ["architecture", "fork", "standalone", "strategic"]
aliases = ["Omegon-standalone"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
archive_reason = "superseded"
archived_at = "1775246517"
dependencies = []
open_questions = []
related = ["rust-agent-loop"]
superseded_by = "ts-to-rust-migration"
+++

# Make Omegon Standalone — Subsume the pi Harness

## Overview

The question: should Omegon stop being a pi-package extension layer and become the authoritative coding agent itself — subsuming the pi harness (TUI, agent loop, OAuth, model routing) internally rather than depending on @mariozechner/pi-coding-agent? This is triggered by upstream PR latency making it impractical to rely on badlogic/pi-mono for bug fixes that directly block Omegon users.

## Research

### Codebase audit — what we'd actually be owning

pi-mono total: ~73k lines across 3 meaningful packages.
Omegon extensions: ~40k lines.

pi-mono breakdown by ownership interest:

COMMODITY — actively developed upstream, expensive to own:
- packages/ai/src/providers/ — 15+ AI provider implementations (Anthropic, OpenAI, Bedrock, Gemini, GitHub Copilot, Vertex, Mistral, Azure, Codex…). New models and API changes land here constantly. This is the highest-churn, highest-risk part to own.
- packages/ai/src/models.generated.ts — auto-generated model registry, regenerated on every upstream release.
- packages/tui/src/ — TUI rendering engine: Unicode column width, editor, kill ring, undo stack, fuzzy matching, stdin buffering, terminal image rendering. Complex, stable, and not something we want to maintain.
- packages/coding-agent/src/core/ — agent loop, session management, bash executor, compaction, tools. Foundational infrastructure.

ISOLATED PAIN POINTS — the actual bugs we care about:
- packages/ai/src/utils/oauth/anthropic.ts — 2 fetch() calls, ~120 lines. Already fixed in PR #2060.
- packages/tui/src/components/input.ts — bracketed paste state machine, ~50 lines touched. Already fixed in PR #2061.
- packages/coding-agent/src/modes/interactive/components/login-dialog.ts — orchestration layer, no bugs here per analysis.

The entire motivation for "owning the upstream" is rooted in 2 small isolated files.

### Option comparison — three paths forward

A: Full standalone — subsume pi-mono into Omegon
  Cost: +73k lines of infrastructure to maintain indefinitely. New AI providers require manual porting. TUI improvements require manual sync. Agent loop security fixes require manual pickup. Essentially become a full coding agent team, not an extensions/workflow team.
  Benefit: Zero upstream dependency. Full control over every seam.
  Risk: HIGH. Ongoing maintenance cost dwarfs the initial bug fixes. Likely to fall behind upstream on model support within months.

B: Patched fork — publish @styrene-lab/pi-coding-agent from cwilson613/pi-mono
  Cost: Maintain a fork with our patches applied. Set up GitHub Actions to publish to npm/GitHub Packages. Periodic upstream sync (rebase or merge). Change Omegon's package.json to reference our scope. Update import paths.
  Benefit: Own only what we need to change. Get upstream AI provider updates, model registry, TUI improvements for free via sync. Our fixes ship immediately without PR approval.
  Risk: LOW-MEDIUM. Fork divergence risk managed by regular upstream syncs. Import path changes are a one-time migration.

C: Status quo + workaround  
  Cost: Users hit login hang until upstream merges PRs. ANTHROPIC_API_KEY env var as workaround. Upstream PR latency continues to be a blocker for future fixes.
  Risk: HIGH operational. Unacceptable.

Verdict: Option B is clearly right. Option A (standalone) solves a governance problem with a 73k-line infrastructure problem — enormous overkill. Option B gives full control over the 2 files that matter while keeping the AI provider ecosystem and TUI for free.

## Decisions

### Decision: Patched fork over full standalone

**Status:** decided

**Rationale:** Full standalone absorption means owning 73k lines of infrastructure (15+ AI providers, TUI rendering engine, agent loop) to fix 2 isolated ~120-line files. The ongoing maintenance cost — porting new model providers, syncing TUI improvements, picking up upstream security fixes — dwarfs the governance benefit. The patched fork (Option B) gives identical control over the specific files that cause problems while getting upstream AI/TUI improvements for free via periodic sync. Publish as @styrene-lab/pi-coding-agent from cwilson613/pi-mono. Migration cost is a one-time import path change in Omegon.
