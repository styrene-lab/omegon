+++
id = "61b82419-a083-472b-a8b7-b04f391f3542"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Auto-delete merged feature branches on OpenSpec archive — Tasks

## 1. extensions/openspec/index.ts (modified)

- [x] 1.1 Extract helper `deleteMergedBranches(pi, cwd, branches: string[]): Promise<{deleted: string[], skipped: string[]}>`:
  - Get current branch via `git rev-parse --abbrev-ref HEAD`
  - Deduplicate `branches` input
  - For each branch: skip if === 'main', 'master', or current HEAD branch
  - Check `git merge-base --is-ancestor <branch> HEAD` — skip (add to skipped) if non-zero exit
  - Run `git branch -d <branch>` — if it fails, add to skipped with no throw
  - Return `{ deleted, skipped }`
- [x] 1.2 In `case "archive"` tool handler (~line 757): after `transitionDesignNodesOnArchive()`, collect `branches[]` from all transitioned design node documents (load each via `scanNodes` or direct file read), call `deleteMergedBranches`, append summary to `result.operations` (e.g. `Deleted branches: feature/foo` or `Skipped unmerged branches: feature/bar`)
- [x] 1.3 In `/opsx:archive` slash-command handler (~line 1678): apply identical branch-cleanup logic after its own `transitionDesignNodesOnArchive()` call — same helper, same output appended to operations list
- [x] 1.4 `resolveBoundDesignNodes` already returns the node objects — use their `.branches` field directly rather than re-reading files

## 2. extensions/openspec/index.test.ts (modified)

- [x] 2.1 Test: branches deleted when `merge-base --is-ancestor` succeeds and `branch -d` succeeds → deleted list populated, skipped empty
- [x] 2.2 Test: branch skipped when `merge-base --is-ancestor` returns non-zero → deleted empty, skipped populated
- [x] 2.3 Test: 'main' and current HEAD branch unconditionally skipped even if merge-base would pass
- [x] 2.4 Test: empty branches array → no git calls, both lists empty (graceful no-op)
- [x] 2.5 Test: duplicate branches deduped → each branch processed only once

## 3. Constraints (verified by tasks above)

- [x] Local only — no `--push --delete` anywhere
- [x] `git branch -d` (safe) not `-D`; failure = skip + continue
- [x] Dedup before processing (task 1.1)
- [x] Skip HEAD branch (task 1.1)
- [x] Skip main/master (task 1.1)
- [x] Append to operations[] (tasks 1.2, 1.3)
- [x] Git ops in index.ts handler, not in spec.ts/archive-gate.ts (task 1.1 helper lives in index.ts)
