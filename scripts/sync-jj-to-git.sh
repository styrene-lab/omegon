#!/usr/bin/env bash
# sync-jj-to-git.sh — Reconcile a jj+git colocated repo before release ops.
#
# Problem:
#   The harness 'commit' tool uses `jj commit`, which writes new commits into the
#   git object store via `jj git export` but does NOT advance refs/heads/main.
#   When `git checkout main` runs afterwards, git moves HEAD back to the old main
#   tip, silently orphaning any jj-only commits from the release.
#
# Solution (must run BEFORE any `git checkout main`):
#   1. Push jj state into git objects (`jj git export`).
#   2. Detect whether jj's working-copy parent (@-) is ahead of git main.
#   3a. Fast-forward: advance refs/heads/main to include the jj commits.
#   3b. Diverged:     error out with instructions — cannot auto-fix.
#   4. Reattach HEAD to main if it is currently detached (safe: main is current).
#
# Usage:
#   Source this script in a bash recipe:
#     . scripts/sync-jj-to-git.sh
#   It is a no-op when jj is not installed or the repo is not jj-colocated.
#
# Environment variables used (read-only):
#   None required.  Reads OLLAMA_HOST etc. only if already set.
#
# Exit codes:
#   0  — jj and git are in sync (or jj is not present)
#   1  — diverged history that cannot be automatically reconciled

if ! command -v jj &>/dev/null 2>&1 || [ ! -d ".jj" ]; then
    # Not a jj-colocated repo — nothing to do.
    exit 0
fi

# ── Step 1: push jj commits into git object store ────────────────────────────
jj git export 2>/dev/null || true

# ── Step 2: read jj and git state BEFORE any checkout ────────────────────────
# '@-' is the parent of the jj working copy — the last *committed* jj revision.
# We use commit_id (full SHA) to avoid ambiguity.
JJ_PARENT=$(jj log --no-graph -r '@-' --template 'commit_id' 2>/dev/null | head -c 40 || true)
MAIN_SHA=$(git rev-parse refs/heads/main 2>/dev/null || true)

if [ -z "$JJ_PARENT" ] || [ -z "$MAIN_SHA" ]; then
    # Could not determine one or both SHAs — skip the reconciliation.
    # This covers: no commits yet, detached jj state, missing main branch.
    true
elif [ "$JJ_PARENT" = "$MAIN_SHA" ]; then
    # Already in sync — nothing to do.
    true
else
    # jj and git main have diverged. Determine if it is a fast-forward.
    if git merge-base --is-ancestor "$MAIN_SHA" "$JJ_PARENT" 2>/dev/null; then
        # ── Fast-forward: jj commits are strictly ahead of git main ──────────
        # Advance refs/heads/main to include them so the release picks them up.
        SHORT_OLD="${MAIN_SHA:0:8}"
        SHORT_NEW="${JJ_PARENT:0:8}"
        echo "⟳  jj→git sync: fast-forwarding main  ${SHORT_OLD} → ${SHORT_NEW}"
        git branch -f main "$JJ_PARENT"
    else
        # ── Non-linear divergence — cannot auto-fix ───────────────────────────
        echo ""
        echo "✗  jj+git divergence detected (non-linear — cannot auto-reconcile)."
        echo ""
        echo "   git main : ${MAIN_SHA:0:12}"
        echo "   jj @-    : ${JJ_PARENT:0:12}"
        echo ""
        echo "   The histories have diverged. Manual resolution required:"
        echo "     Inspect : jj log -r 'ancestors(@, 10)'"
        echo "     Include  : jj bookmark set main -r @-   (advances main)"
        echo "     Abandon  : jj abandon @-                (discards jj commits)"
        echo ""
        exit 1
    fi
fi

# ── Step 3: reattach HEAD to main if currently detached ──────────────────────
# After jj git export, git HEAD is often detached at the jj working-copy commit.
# main now points to that commit (or was already there), so checkout is a no-op
# in terms of file content but makes git branch --show-current return "main".
if [ -z "$(git branch --show-current 2>/dev/null)" ]; then
    git checkout main 2>/dev/null || true
fi
