+++
id = "36881f1a-d86e-432a-a1a2-8146dddab67a"
kind = "document"
title = "Auto-delete merged feature branches on OpenSpec archive"
status = "implemented"
tags = ["openspec", "git", "lifecycle", "cleanup"]
aliases = ["archive-branch-cleanup"]
imported_reference = false

[publication]
enabled = false
visibility = "private"

[data]
branches = []
open_questions = []
openspec_change = "archive-branch-cleanup"
+++

# Auto-delete merged feature branches on OpenSpec archive

## Overview

After /opsx:archive completes, the archive handler already transitions design nodes to implemented. It should also delete any git branches recorded in those nodes' branches[] field that are fully merged into the current branch. This closes the loop: spec archived → branches gone, with no manual cleanup needed. The handler already has pi in scope for pi.exec. Safety check: git merge-base --is-ancestor before any deletion. Only deletes local branches; does not touch remotes.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/openspec/index.ts` (modified) — In case 'archive': after transitionDesignNodesOnArchive(), collect branches[] from all transitioned design nodes, verify each is fully merged (git merge-base --is-ancestor <branch> HEAD), delete local branch via git branch -d. Append deleted/skipped counts to result.operations. Handle the /opsx:archive slash-command path at line ~1678 identically.
- `extensions/openspec/index.test.ts` (modified) — Add tests: branch deleted when fully merged; branch skipped when not merged (--is-ancestor fails); no branches field on node (graceful no-op); empty branches array (no-op).
- `extensions/openspec/branch-cleanup.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes
- `extensions/openspec/branch-cleanup.test.ts` (modified) — Post-assess reconciliation delta — touched during follow-up fixes

### Constraints

- Only delete local branches — never push --delete to origin
- Use git branch -d (safe delete) not -D; if it fails (unmerged), log skip and continue
- Collect branches[] from ALL transitioned nodes, deduplicate, then process
- Skip the current branch (HEAD) — never delete the branch you're on
- Skip 'main' and 'master' unconditionally regardless of merge status
- If pi.exec is not available in the pure-function context, perform git ops in the tool handler (index.ts) not in archive-gate.ts or spec.ts
- Add result to operations[] so the archive renderResult shows e.g. 'Deleted branches: feature/foo, feature/bar'
