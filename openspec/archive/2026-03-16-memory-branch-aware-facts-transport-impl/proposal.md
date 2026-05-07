+++
id = "4d5b61bd-b5b4-4f3e-b983-f7d1b38fed1f"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Implement branch-aware memory transport for facts.jsonl

## Intent

Implement the decided branch-aware facts transport design: keep startup import from .pi/memory/facts.jsonl, stop unconditional shutdown export, add explicit export/drift helpers, surface memory transport drift separately from hard lifecycle blockers, and update tests/docs.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
