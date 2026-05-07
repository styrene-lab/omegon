+++
id = "e5876704-f550-4e35-81bd-b180001ba54d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Compaction policy hotfix for heavy local models and temp-path hygiene

## Intent

Immediately stop harmful local-first compaction defaults that pick heavy Ollama models for routine compaction, prefer cloud/fallback-safe behavior, and sanitize ephemeral pi-clipboard temp image paths from compaction summaries. Follow with proper OpenSpec artifacts so the fix is specified before implementation changes are finalized.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
