+++
id = "d729f35a-84ea-4049-92f8-24a490fda15c"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auto-delete merged feature branches on OpenSpec archive — Design

## File Changes

- `extensions/openspec/index.ts` (modified) — In case 'archive': after transitionDesignNodesOnArchive(), collect branches[] from all transitioned design nodes, verify each is fully merged (git merge-base --is-ancestor <branch> HEAD), delete local branch via git branch -d. Append deleted/skipped counts to result.operations. Handle the /opsx:archive slash-command path at line ~1678 identically.
- `extensions/openspec/index.test.ts` (modified) — Add tests: branch deleted when fully merged; branch skipped when not merged (--is-ancestor fails); no branches field on node (graceful no-op); empty branches array (no-op).

## Constraints

- Only delete local branches — never push --delete to origin
- Use git branch -d (safe delete) not -D; if it fails (unmerged), log skip and continue
- Collect branches[] from ALL transitioned nodes, deduplicate, then process
- Skip the current branch (HEAD) — never delete the branch you're on
- Skip 'main' and 'master' unconditionally regardless of merge status
- If pi.exec is not available in the pure-function context, perform git ops in the tool handler (index.ts) not in archive-gate.ts or spec.ts
- Add result to operations[] so the archive renderResult shows e.g. 'Deleted branches: feature/foo, feature/bar'
