+++
id = "377b078d-bfc1-4b53-9d99-baf026654f36"
title = "Release regression suite reconciliation"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/Known Issues"
imported_at = "2026-07-13T19:31:36.449697Z"
imported_reference = true
kind = "memory_fact"
topic = "Known Issues"

[publication]
enabled = false
visibility = "private"

+++

Release regression suite reconciled in commit 4121fe35. Fixed two actual TUI defects: `/extension restart` now queues the same graceful process restart as `/runtime restart`, and runtime/skills reload now projects newly loaded skill activation events into conversation segments. Updated stale tests for session-local numeric plan IDs, compact footer/Workbench ownership, selected-segment marker placement, current 29 AgentEvent variants, and permission trust assertions robust to the operator's active profile. `cargo test -p omegon --locked` passed 3951 unit + all blackbox tests; `just lint` passed. `just test-rust` progressed through all Omegon and most workspace crates cleanly but the old pre-rebuild styrene-work-model test binary stalled in dyld; rerunning `cargo test -p styrene-work-model -- --test-threads=1` rebuilt it and passed 2/2. `just link` installed the release binary.
