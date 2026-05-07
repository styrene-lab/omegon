+++
id = "db3067a8-cbf8-4053-b7ca-f2bf927cd1db"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Subagent architecture — map cleave onto the subagent mental model with Omegon-native advantages

## Intent

The industry has converged on \"subagents\" as the developer mental model for multi-agent work: a parent agent invokes specialist children for focused tasks. Claude Code, OpenCode, Codex CLI, Spring AI — all use this pattern. Omegon has cleave, which is more powerful (git worktrees, merge policies, adversarial review, scope isolation) but maps poorly to this mental model because it's batch-oriented (plan → split → execute all → merge) rather than on-demand (working → need help → invoke specialist → get result → continue).\n\nThe opportunity: expose cleave's infrastructure through the subagent UX pattern, giving developers the familiar interaction model with Omegon's superior execution guarantees.

See [design doc](../../../docs/subagent-architecture.md).
