# Contributing to pi-kit

Guidelines for branching, merging, and collaborating on this repository.

## Development Model

**Trunk-based development** on `main`. Direct commits for small, self-contained changes. Feature branches for multi-file or multi-session work.

### When to Branch

| Scenario | Approach |
|---|---|
| Single-file fix, typo, config tweak | Commit directly to `main` |
| Multi-file feature or refactor | `feature/<name>` or `refactor/<name>` branch |
| Multi-session work (spans days) | Feature branch, push regularly |
| Cleave-dispatched parallel tasks | Automatic `cleave/*` worktree branches (ephemeral) |

### Branch Naming

Follow `<type>/<short-description>` per the [git skill](skills/git/SKILL.md):

```
feature/design-tree
fix/memory-zombie-resurrection
refactor/rename-diffuse-to-render
chore/bump-dependencies
```

### Merging

- **Merge commits** (not squash, not rebase) for feature branches — preserves full history
- **Fast-forward** is fine for single-commit branches
- **Never rebase branches that touch `facts.jsonl`** — see [Memory Sync](#memory-sync) below
- Delete the branch after merge (local and remote)

## Commits

[Conventional Commits](https://www.conventionalcommits.org/) required. See [git skill](skills/git/SKILL.md) for the full spec.

```
feat(project-memory): add union merge strategy for facts.jsonl
fix(cleave): bare /assess runs adversarial session review
docs: add contributing guide and branching policy
```

Commit messages explain *why*, not just *what*. Include the motivation in the body when the subject line isn't self-evident.

## Memory Sync

The project memory system uses a three-layer architecture for cross-machine portability:

```
facts.db (SQLite)     ← runtime working store (local, .gitignored)
facts.jsonl (JSONL)   ← transport format (git-tracked, union merge)
content_hash (SHA256) ← dedup key (idempotent import)
```

### How It Works

1. **Session start**: `facts.jsonl` is always imported into `facts.db`. Dedup by `content_hash` makes this safe to run every session — existing facts get reinforced, new ones inserted, archived/superseded ones skipped.

2. **Session shutdown**: Active facts, edges, and episodes are exported from `facts.db` to `facts.jsonl`, overwriting the file.

3. **Git merge**: `.gitattributes` declares `merge=union` for `facts.jsonl`. On merge, git keeps all lines from both sides, removing only exact duplicates. Redundant lines are harmlessly deduplicated at next import.

### Rules

| Rule | Reason |
|---|---|
| Never manually edit `facts.jsonl` | Machine-generated; manual edits will be overwritten on next session shutdown |
| Never rebase across `facts.jsonl` changes | `merge=union` only works with merge commits; rebase replays one side's version, losing the other's facts |
| Never `git checkout -- facts.jsonl` to resolve conflicts | Use `merge=union` (automatic) or manual union: keep all lines from both sides |
| Don't track `*.db` files | Binary, machine-local, rebuilt from JSONL on session start |

### .gitignore / .gitattributes

```
# .pi/.gitignore — exclude runtime DB files
memory/*.db
memory/*.db-wal
memory/*.db-shm

# .gitattributes — union merge for append-log JSONL
.pi/memory/facts.jsonl merge=union
```

## Cleave Branches

The [cleave extension](extensions/cleave/) creates ephemeral worktree branches for parallel task execution:

```
cleave/<childId>-<label>    # e.g., cleave/a1b2c3-fix-imports
```

These branches are:
- Created automatically by `cleave_run`
- Merged back to the parent branch sequentially
- Worktree directories cleaned up after merge
- **Branches preserved on merge failure** for manual resolution

### Cleanup

After cleave completes successfully, worktree directories are pruned but branches may linger. Clean up periodically:

```bash
# Delete local branches already merged into main
git branch --merged main | grep 'cleave/' | xargs -r git branch -d

# Prune remote tracking refs for deleted remote branches
git fetch --prune
```

## Repository Hygiene

### Stale Branches

Delete remote branches after merge. Don't accumulate tracking refs:

```bash
# List remote branches merged into main
git branch -r --merged origin/main | grep -v 'main$'

# Delete a stale remote branch
git push origin --delete <branch-name>
```

### Protected Files

Files that should never cause merge conflicts due to their nature:

| File | Strategy | Notes |
|---|---|---|
| `.pi/memory/facts.jsonl` | `merge=union` | Append-log, deduped at import |
| `*.db`, `*.db-wal`, `*.db-shm` | `.gitignore` | Binary, machine-local |
| `.pi/memory/` directory | Partial ignore | Only `facts.jsonl` tracked |

### What Gets Tracked

```
✅ Tracked                          ❌ Ignored
─────────────────────────────────   ─────────────────────────
extensions/**/*.ts                  node_modules/
skills/**/SKILL.md                  *.db, *.db-wal, *.db-shm
prompts/*.md                        .env
themes/*.json                       .claude/
.pi/memory/facts.jsonl              .DS_Store
.gitattributes                      bin/rg, bin/fd
package.json
```

## Scaling Notes

This policy is designed for a small team (1–3 contributors) working with agent-assisted development. If the contributor count grows:

- Enable branch protection on `main` (require PR, at least 1 review)
- Add CI validation for conventional commits (`commitlint`)
- Consider a `develop` branch if release cadence requires staging
- Monitor `facts.jsonl` size — if it exceeds ~10K lines, evaluate archival rotation or LFS
