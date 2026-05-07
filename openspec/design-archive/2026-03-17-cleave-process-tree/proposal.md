+++
id = "c75bb9a6-f3c7-4adb-a8f2-397499d92a77"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave Process Tree — bidirectional parent↔child coordination

## Intent

Replace cleave's current fire-and-forget task-file protocol with bidirectional parent↔child communication. Children are trusted subprocesses spawned by Omegon — no discovery, no auth, no HTTP overhead. The goal is enabling mid-task negotiation (child asks parent for input), sibling awareness (children know what others have done), structured progress (richer than stdout line scraping), and coordinated resource access (shared file locks, interface contracts).

See [design doc](../../../docs/cleave-process-tree.md).
