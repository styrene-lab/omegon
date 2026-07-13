+++
id = "0af82e53-7f06-4d41-b4d0-9edfbdfd191a"
title = "Profile selection persistence and menu truncation fix"
tags = []
aliases = []
source_format = "omegon_memory"
source_path = "omegon://memory/Known Issues"
imported_at = "2026-07-13T18:36:01.840569Z"
imported_reference = true
kind = "memory_fact"
topic = "Known Issues"

[publication]
enabled = false
visibility = "private"

+++

Profile project-scope selection bug fixed in commit 8ff8f3c0. Root cause: project_active_profile_path_from_registry treated legacy `.omegon/profile.json` like a registry file and walked two parents, resolving `active-profile.json` at repo root instead of `.omegon/active-profile.json`; global user pointer could then remain authoritative. Resolution distinguishes RegistryFile and LegacySingleton paths, preserves project legacy precedence over global selection, and makes menu summary/footer wrap with dynamic body budgeting. Shared command modal grew to 120x32. Focused settings/menu/profile tests and lint pass; full cargo test still has 12 pre-existing unrelated failures in conversation plan IDs, old TUI footer expectations/runtime aliases/skill event, conv widget marker, and WS event count.
