+++
id = "51884cd4-a911-4ee2-b77b-8db5eefb83ce"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# ai/ directory convention — unified agent artifact home

## Intent

Adopt the emerging ai/ directory convention as the home for all agent-specific artifacts: design docs, OpenSpec changes, memory facts, lifecycle state. Currently scattered across docs/, openspec/, .omegon/memory/, .omegon/lifecycle/. The ai/ folder is visible, version-controlled, and semantically clear — it says 'this is agent-managed content' without hiding behind dotfiles.\n\nThe .omegon/ dotfile remains for tool configuration only (profile.json, tutorial state, calibration).\n\nWhen we encounter an existing project with an ai/ directory, we can enrich it with our more robust conventions (design tree, OpenSpec, memory, milestones).

See [design doc](../../../docs/ai-directory-convention.md).
