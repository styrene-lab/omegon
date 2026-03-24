---
id: blattman-pattern-harvest
title: Assessment — Harvestable patterns from claudeblattman.com
status: exploring
source: https://claudeblattman.com/
date: 2026-03-24
tags: [assessment, patterns, skills, context-management, ux]
---

# Blattman Pattern Harvest for Omegon

Assessment of every extractable pattern from claudeblattman.com's AI tooling system (skills, agents, workflows, context management, voice, continuous improvement) mapped against Omegon's existing architecture. Categorized by where the value lands.

---

## 🟢 HARVEST — Directly applicable to the Omegon harness

These patterns can be codified in the harness itself. They're structural, not domain-specific.

### 1. Skill Size Budgeting

**Source:** "No skills currently exceed 5K words (largest: triage-reminders at 3,703 words)"

**Insight:** Large skills eat context budget and dilute attention. Blattman decomposed a 5K+ word skill into 1,563-word core + 4 reference files.

**Omegon mapping:** Omegon loads SKILL.md files into context when a task matches. No size guard exists. The harness should:
- Warn at skill load time if a skill exceeds ~4K words
- Suggest decomposition into a core skill + reference files
- Track skill token cost in the `/status` panel

**Effort:** Low. Instrumentation only.

---

### 2. Skill Versioning

**Source:** "All 22 active skills now have `*v1.0 — date — description*` version lines"

**Insight:** Skills drift. Without version tracking, it's impossible to know if a loaded skill is current.

**Omegon mapping:** Omegon's SKILL.md files have no version convention. Add:
```
*v1.2 — 2026-03-24 — Added cache-first keyring resolution*
```
Single line after title, overwrite (not append) on each change. The harness can parse this for staleness detection.

**Effort:** Convention only. Optional parse for staleness warnings.

---

### 3. Config Externalization (Data ≠ Workflow)

**Source:** Skills read policy/config files before acting. "Config is the source of truth; the skill body describes *workflow*, not *data*."

**Insight:** Skills shouldn't hardcode domain knowledge. Blattman uses `policies/` and `config/` directories.

**Omegon mapping:** Omegon already separates data from workflow via:
- Memory facts (project knowledge)
- AGENTS.md (operator directives)
- Persona mind facts (identity data)

**Gap:** No formalized per-skill config directory. A skill that needs VIP lists, classification rules, or thresholds currently has to embed them or read from memory. Consider supporting a `skills/<name>/config/` directory convention where skills can stash externalized rules.

**Effort:** Medium. Convention + loader change.

---

### 4. Tool Limitation Self-Awareness

**Source:** "Claude should proactively recommend other tools when they would serve the task better."

**Insight:** The agent should know what it's bad at and say so unprompted.

**Omegon mapping:** The harness should inject a `tool-limitations.md` (or memory facts) that the agent consults when:
- Asked to do spreadsheet work → suggest Excel/Sheets
- Asked to do real-time collaboration → suggest the actual doc
- Asked for video/audio analysis → suggest Gemini
- Asked for image generation → suggest DALL-E/Midjourney directly

This is harness-level because it's about the agent loop's self-model, not any specific persona.

**Effort:** Low. A static reference file injected into system prompt, or memory facts.

---

### 5. Graceful Failure with Recovery Suggestions

**Source:** "When something goes wrong (missing file, MCP not connected), the skill should explain what happened and suggest a fix — not crash silently."

**Insight:** Every failure mode needs a recovery path visible to the operator.

**Omegon mapping:** Omegon's startup probes already do this (⚠ anthropic → /login). Extend the pattern:
- MCP server disconnect → "Run `/mcp reconnect <name>` or check the process"
- Memory DB corruption → "Run `/memory rebuild`"  
- Tool execution timeout → "Increase timeout with `/config tool-timeout 60s`"
- Provider rate limit → "Switching to fallback tier. Use `/tier` to override"

Formalize as a `recovery_hint` field on error responses from the tool executor.

**Effort:** Medium. Needs error categorization + hint mapping.

---

### 6. Default to Action (Zero-Arg Commands)

**Source:** "Skills should do something useful with no arguments."

**Insight:** Every slash command should have a sensible default behavior when invoked bare.

**Omegon mapping:** Already good: `/dash` opens dashboard, `/login` opens selector, `/status` shows panel. Audit remaining commands:
- `/secrets` → should list (already does ✓)
- `/tutorial` → should start demo (already does ✓)  
- `/assess` → should assess current working tree (currently requires subcommand)
- `/memory` → should show summary (currently requires subcommand)

**Effort:** Low. Audit + add defaults where missing.

---

### 7. Confirmation Permission Matrix

**Source:** "DO ask before: [destructive things]. DO NOT ask before: [routine things]. When in doubt: do it."

**Insight:** Operators lose flow when asked to confirm routine actions. The set of "destructive" vs "routine" should be configurable.

**Omegon mapping:** Omegon's approval system is inherited from pi. The harness could add an operator-configurable matrix:
```json
// ~/.omegon/settings.json
{
  "auto_approve": ["file_read", "file_edit", "bash_readonly"],
  "always_confirm": ["git_push", "file_delete", "bash_destructive"],
  "auto_approve_patterns": ["cargo test", "cargo build", "just *"]
}
```

**Effort:** Medium-high. Needs approval hook integration.

---

### 8. Prompt Cache Coherence

**Source:** "Prompt Caching Architecture — Lessons from Building Claude Code"

**Insight:** System prompt content that stays stable across turns gets cached by the API. Reordering or mutating injected content breaks cache.

**Omegon mapping:** Omegon injects AGENTS.md + skill files + memory facts into the system prompt. If memory fact injection order is unstable across turns (e.g., different semantic recall results), cache hit rate drops. The harness should:
- Sort injected memory facts deterministically (by ID, not by relevance score)
- Put stable content (AGENTS.md, skills) first, dynamic content (recalled facts) last
- Track cache hit rate as a diagnostic metric

**Effort:** Medium. Requires injection ordering audit + observability.

---

### 9. Deferred Investigation Queue (@ToSelf)

**Source:** "@ToSelf labels for deferred investigation, processed in batch by /todo-review"

**Insight:** Mid-session, the agent notices something worth investigating later but shouldn't context-switch now. Needs a lightweight capture mechanism.

**Omegon mapping:** The design tree has `deferred` status, but creating a full design node for "check if this test is flaky" is heavyweight. Add:
- `/note <text>` — appends to `.omegon/notes.md` with timestamp
- `/notes` — shows pending notes  
- At session start, inject note count: "You have 3 pending notes from previous sessions"

Lighter than design nodes, heavier than memory facts. Think of it as the agent's scratch pad that persists.

**Effort:** Low. File append + session start injection.

---

### 10. Interactive Triage / Checkin Command

**Source:** `/checkin` is "an interactive session — it triages your inbox, triages your reminders, preps meetings, drafts emails, and surfaces priorities"

**Insight:** A structured start-of-session triage that processes accumulated state.

**Omegon mapping:** `/checkin` for Omegon would:
- Show design tree: ready queue (decided + unblocked), blocked nodes with blockers
- Show pending notes (from @ToSelf / `/note`)
- Show git status: uncommitted changes, unpushed commits
- Show failing tests (if test runner configured)
- Show stale memory facts (not accessed in N sessions)
- Show OpenSpec changes in progress

This is the operational inverse of the splash screen — splash shows *capabilities*, checkin shows *state that needs attention*.

**Effort:** Medium. Aggregation command pulling from multiple subsystems.

---

### 11. Priority Alignment View

**Source:** `/goals-review` aligns daily work to quarterly objectives

**Insight:** The agent should know what the operator cares about *this quarter* and nudge toward it.

**Omegon mapping:** The design tree already has priorities (1-5) and issue types. Add:
- `/priorities` — shows P1-P2 nodes from design tree, sorted by readiness
- Optional: `~/.omegon/goals.yaml` with quarterly objectives, referenced in checkin
- Agent behavior: when a cleave plan or new work is proposed, flag if it doesn't connect to a P1-P2 goal

**Effort:** Low-medium. Design tree query + optional goals file.

---

### 12. Parallel Agent Dispatch (Adversarial Review)

**Source:** "Launch multiple agents simultaneously for different review dimensions"

**Insight:** Run N reviewers in parallel, each with different concerns (security, performance, correctness, style), merge their findings.

**Omegon mapping:** Cleave already does parallel child dispatch. The `/assess cleave` reviewer runs a single adversarial pass. Extend to support multiple specialized reviewers:
- Security reviewer (load security skill)
- Performance reviewer (load perf heuristics)
- Spec compliance reviewer (load OpenSpec scenarios)

Each gets a fresh context (like Blattman's agent isolation). Findings merged and deduplicated.

**Effort:** High. Needs reviewer persona parameterization in cleave.

---

## 🟡 PERSONA TERRITORY — Ship as persona/skill packs, not in the harness

These patterns are valuable but domain-specific. They belong in publishable persona packs.

### 13. Executive Assistant Persona

Blattman's full EA workflow: `/morning-brief`, `/checkin` (email-specific), `/triage-inbox`, VIP tracking, calendar management. This is a complete persona with:
- Mind facts: VIP list, calendar IDs, email rules, working hours
- Skills: inbox triage, meeting prep, email drafting
- Tone: professional, concise, proactive
- MCP dependencies: Gmail, Google Calendar, Apple Reminders

**Ship as:** `persona-executive-assistant` package with bundled skills and example config.

### 14. Voice Pack Framework

Blattman's voice pack template (spec, examples, testing, maintenance) is a formalized approach to tone configuration. Omegon already has `ToneSummary` with intensity modes.

**Ship as:** A `docs/authoring-voice-packs.md` guide for persona authors. The template structure (register, examples, anti-patterns, test cases) becomes the standard for all tone definitions.

### 15. Research/Academic Persona

Proposal writing, literature review, data analysis skills, journal-specific formatting — these are domain skills.

**Ship as:** `persona-researcher` package.

### 16. Project Manager Persona

Weekly reviews, project dashboards, timeline tracking, stakeholder updates.

**Ship as:** `persona-project-manager` package.

---

## 🔴 OUT OF SCOPE — Higher level or not applicable

### 17. CLAUDE.md Template
Omegon uses AGENTS.md with a different architecture (hierarchical: global → project). The template content is useful for tutorial documentation ("here's how to write your AGENTS.md") but doesn't change the harness.

### 18. Claude Code Internals Documentation
Blattman's 16,000-word "Under the Hood" documents Claude Code's message stack. Omegon IS the agentic loop — we document our own internals, not theirs. However, the *context window economics* insights (which tokens are cached, cost per turn, etc.) are worth reading for our own optimization work.

### 19. Skill Marketplace / Distribution
"Share what works — Publish skills that might help others" implies a registry. This is a platform concern for a future Omegon version, not an rc.40 harness feature.

### 20. VS Code / IDE Integration
Blattman's VS Code setup is about Claude Code's editor integration. Omegon is TUI-first. IDE integration would be a separate extension effort, probably post-1.0.

### 21. "ToolSearch" Avoidance
Blattman's skills say "DO NOT use ToolSearch" because pre-approved tools are already loaded. Omegon's tool profiles already solve this — manage_tools controls what's visible. No change needed.

---

## Priority Matrix

| # | Pattern | Effort | Impact | Priority |
|---|---------|--------|--------|----------|
| 6 | Zero-arg command audit | Low | Medium | **P1 — do now** |
| 9 | `/note` deferred queue | Low | High | **P1 — do now** |
| 1 | Skill size budgeting | Low | Medium | **P2 — next sprint** |
| 2 | Skill versioning | Low | Low | **P2 — next sprint** |
| 4 | Tool limitation self-awareness | Low | High | **P2 — next sprint** |
| 5 | Graceful failure recovery | Medium | High | **P2 — next sprint** |
| 10 | `/checkin` triage command | Medium | High | **P2 — next sprint** |
| 11 | `/priorities` view | Low-Med | Medium | **P3 — backlog** |
| 8 | Prompt cache coherence | Medium | Medium | **P3 — backlog** |
| 3 | Per-skill config dirs | Medium | Low | **P3 — backlog** |
| 7 | Confirmation matrix | Med-High | Medium | **P3 — backlog** |
| 12 | Multi-reviewer dispatch | High | High | **P4 — design phase** |

---

## Summary

**12 harvestable patterns** for the harness, **4 persona-territory patterns** that should ship as packaged personas/guides, **5 out-of-scope** items.

The highest-value, lowest-effort wins: `/note` command for deferred investigation, zero-arg audit on remaining commands, and a `tool-limitations.md` for agent self-awareness. The biggest structural improvement would be `/checkin` as the operational counterpart to the splash screen.

The persona framework patterns (voice packs, EA workflow, research skills) validate that Omegon's existing persona/tone architecture is on the right track — it just needs published examples and an authoring guide.
