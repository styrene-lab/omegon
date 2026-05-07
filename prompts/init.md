+++
id = "9d43af85-05b0-4f56-b21b-8ab39302eb1d"
tags = []
aliases = []
imported_reference = false

[publication]
enabled = false
visibility = "private"
+++

# Project Init

You are orienting to a project directory for the first time. Perform each step, then present a concise summary.

## 1. Environment Scan

```bash
pwd
ls -la
```

Check if this is a git repository:
```bash
git rev-parse --is-inside-work-tree 2>/dev/null && echo "GIT_REPO=yes" || echo "GIT_REPO=no"
git remote -v 2>/dev/null
git log --oneline -5 2>/dev/null
```

Detect project type (look for key files):
```bash
ls package.json pyproject.toml Cargo.toml go.mod Makefile Dockerfile *.sln 2>/dev/null
```

## 2. Initialize Memory

Use `memory_query` to check if project memory already exists.

- If **no facts exist**: this is truly a first session. Read key files (README, config files, project manifests) to understand the project. Use `memory_store` to persist 3-5 foundational facts about the project (language, structure, key abstractions).
- If **facts exist**: this project has prior context. Skim the facts and skip to step 3.

## 3. Check Tooling State

Use `design_tree` action `list` to see if any design explorations exist.

Check for design doc migration needs:
```bash
# If design docs with frontmatter (id/status) exist in docs/ instead of docs/design/,
# suggest running /migrate to archive completed explorations
ls docs/design/ 2>/dev/null || echo "NO_DESIGN_ARCHIVE"
```

If `docs/design/` doesn't exist but `docs/` has markdown files with design-tree frontmatter, note that `/migrate` is available to archive completed design docs.

Check for OpenSpec changes:
```bash
ls openspec/changes/ 2>/dev/null
```

Check for active branches:
```bash
git branch --list 2>/dev/null | head -10
```

## 4. Present Summary

Give the operator a **brief, scannable summary** (not a wall of text):

```
📍 <directory name> — <one-line description>
   <language/framework> · <git status or "not a git repo">

🧠 Memory: <N facts | fresh start>
🌳 Design: <N nodes | none>
📋 OpenSpec: <N active changes | none>
🔀 Branch: <current branch>

<If migration available: "📦 /migrate available — N design docs can be archived to docs/design/">
<If first session: "Ready to explore. What are we building?">
<If returning: "Welcome back. Pick up where we left off?">
```

Keep the summary under 10 lines. Do NOT dump file listings or memory contents — just the counts and orientation cues.
