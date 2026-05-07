+++
id = "341e8311-3c05-4120-bedf-165b58a90afb"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Skill-Aware Child Dispatch

## Problem
Cleave child processes are generic agents with no guidance on which skills matter for their task. Local models especially struggle with self-selection. All children run at the same model tier regardless of task complexity. There is no automated review loop — quality assurance requires manual `/assess` invocation after merge.

## Solution
1. **Skill matching** — Auto-match skills from child scope (file patterns) with `<!-- skills: ... -->` annotation override. Inject skill read directives into child prompts.
2. **Tiered execution** — Route children to appropriate model tiers (local/haiku/sonnet/opus) based on skill complexity hints and scope analysis.
3. **Review loop** — After execution, an adversarial review agent (opus-tier) evaluates the child's work against spec scenarios. Severity-gated: nits=pass, warnings=1 fix, critical=2 fixes then escalate. Diminishing returns guardrail bails on >50% issue reappearance.

## Design Reference
`design/skill-aware-child-dispatch.md` — all decisions D1–D4c finalized.

## Scope
- `extensions/cleave/skills.ts` (new) — skill matching, scope-to-skill mapping
- `extensions/cleave/review.ts` (new) — review loop, adversarial prompt, severity parsing, churn detection
- `extensions/cleave/dispatcher.ts` — wire review loop into dispatch, model selection per child
- `extensions/cleave/workspace.ts` — inject skill directives into child prompts
- `extensions/cleave/types.ts` — add skills/executeModel to ChildPlan, review types
- `extensions/cleave/index.ts` — expose review config, wire new options into cleave_run
- `extensions/cleave/openspec.ts` — parse `<!-- skills: ... -->` annotations
- `skills/cleave/SKILL.md` — document skill annotations and review loop behavior
