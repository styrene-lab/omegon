+++
id = "b8641356-e413-4e5b-b9b0-aa0a043adc7a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Lifecycle Reconciliation — Ambient Sync from Implementation Reality back to Design Tree + OpenSpec

## Intent

Make lifecycle tracking bidirectional. Today the flow from design tree → OpenSpec → cleave works well, but agent-driven implementation does not automatically reconcile tasks, node status, and dashboard state as reality changes. Add ambient reconciliation points so implementation progress updates design-tree and OpenSpec continuously and reliably without manual bookkeeping.
