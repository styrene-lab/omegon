+++
id = "46cbfffc-0167-4810-983e-4a36a1029283"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# ai/ directory convention — unified agent artifact home — Design Spec (extracted)

> Auto-extracted from docs/ai-directory-convention.md at decide-time.

## Decisions

### ai/ is the unified agent artifact home (decided)

Current layout: design docs in docs/, OpenSpec in openspec/, memory in .omegon/memory/, lifecycle in .omegon/lifecycle/, milestones in .omegon/milestones.json. All scattered. The ai/ convention is emerging in the wild as the standard place for agent-managed content. Moving everything under ai/ makes it obvious what the agent touches, what's version-controlled agent work, and lets us enrich existing ai/ folders with our robust conventions. Layout: ai/docs/ (design tree), ai/openspec/ (lifecycle), ai/memory/ (facts), ai/lifecycle/ (opsx state), ai/milestones.json. The .omegon/ dotfile stays for tool config only: profile.json, tutorial state, calibration, agents/. AGENTS.md stays at repo root (it's a project convention file like .gitignore, not an agent artifact).
