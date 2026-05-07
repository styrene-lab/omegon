+++
id = "47db0ef7-b434-42be-8155-6ac07b798f28"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Model selector UX — browse and pick, don't memorize

## Overview

/model command should not require operators to know exact model IDs from memory. Instead, offer an interactive selector that:

1. **Provider picker** (if needed): Which inference service? (anthropic, openrouter, groq, etc.)
2. **Model browser**: Sorted list with:
   - Human name + model ID
   - Context limits (input/output tokens)
   - Speed/cost tier or pricing info
   - Brief descriptor (reasoning model, ultra-fast, free-tier eligible, etc.)
3. **Search/filter**: Type to narrow by name or capability
4. **Select**: Arrow keys or number selection, then saves to session

Current state: `/model openrouter:model-id` requires memorized IDs — not discoverable.

Related to: unified-auth-surface (after login, user needs to pick a model), tui-surface-pass (needs TUI widget integration).
