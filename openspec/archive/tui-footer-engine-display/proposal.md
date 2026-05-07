+++
id = "a945f83d-896a-4f9c-8286-42170bdcce7f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Footer redesign — engine display + linked minds

## Intent

Merge the current 4-card footer into a denser, more meaningful layout:

**Engine panel** (replaces context + model cards): Unified display of the tri-axis — provider/tier/thinking. Shows the "engine configuration" as a single coherent unit. Context gauge stays but is part of this panel. Model name, tier badge, thinking level indicator all in one visual group.

**Minds panel** (replaces memory card): "Linked minds" concept — which memory systems are active (project memory, working memory, episodes, archive). Each mind shows: name, fact count, injection status, estimated token weight. The headline is the active minds, not a raw fact count.

**System panel** (remains but leaner): cwd, git branch (just current, not the full tree — that goes to sidebar), session uptime, MCP status. Tool call and compaction counters move here.
