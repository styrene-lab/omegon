+++
id = "4eb109bc-740f-46ed-a1a6-99d681b23278"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Cleave checkpoint execution reliability and failure clarity

## Intent

Ensure the cleave checkpoint path is trustworthy at the dirty-tree boundary: a confirmed checkpoint must either create the intended checkpoint and allow execution to continue, or return an explicit failure diagnosis explaining why the worktree remains dirty. This should cover both cleave entry preflight and any equivalent merge-back/inverted checkpoint path.

## Scope

<!-- Define what is in scope and out of scope -->

## Success Criteria

<!-- How will we know this change is complete and correct? -->
