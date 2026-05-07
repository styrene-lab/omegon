+++
id = "352b915e-ea21-4e43-910c-7067d487872a"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Squash-merge policy for feature branches — Design Spec (extracted)

> Auto-extracted from docs/squash-merge-policy.md at decide-time.

## Decisions

### Cleave orchestrator uses git2 merge --squash for child branches instead of merge --no-ff (decided)

Child diary commits (edit, fix test, re-edit) have no value on main. Squash-merge produces one clean commit per child with the child's label and description as the message. The diary stays on the branch until cleanup. git2's merge + index + commit API supports this natively. For interactive feature branches, the harness should offer squash-merge when the operator closes a branch.
