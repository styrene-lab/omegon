#!/usr/bin/env bash
# milestone-update.sh — Maintain .omegon/milestones.json during release lifecycle.
#
# Called by release and nightly automation to keep milestone state in sync
# with the actual release cadence. Designed so that milestones.json is
# always the source of truth for "what version are we building toward,
# what shipped, and what's in flight."
#
# Usage:
#   milestone-update.sh release  <version>   # Mark milestone released (e.g. 0.15.3)
#   milestone-update.sh open     <version>   # Open next milestone after stable release
#   milestone-update.sh nightly  <version>   # Record a nightly build (e.g. 0.15.3-nightly.20260326)

set -euo pipefail

ACTION="${1:?Usage: milestone-update.sh <release|open|nightly> <version>}"
VERSION="${2:?Usage: milestone-update.sh <release|open|nightly> <version>}"

MILESTONES_FILE=".omegon/milestones.json"
mkdir -p .omegon

# Extract the base version (strip -nightly.DATE)
base_version() {
    echo "$1" | sed -E 's/-nightly\.[0-9]+$//'
}

# Get current ISO 8601 timestamp
now_iso() {
    date -u +"%Y-%m-%dT%H:%M:%SZ"
}

# Collect commit subjects since last tag (for notes)
collect_notes() {
    local base="$1"
    # Find the previous tag to diff against
    local prev_tag
    prev_tag=$(git tag --sort=-v:refname | grep -v "^v${base}" | head -1 2>/dev/null || echo "")
    if [ -z "$prev_tag" ]; then
        # No previous tag — use all commits
        prev_tag=$(git rev-list --max-parents=0 HEAD | head -1)
    fi
    # Collect feat/fix commits as notes
    git log "${prev_tag}..HEAD" --oneline --no-merges \
        --grep="^feat" --grep="^fix" --grep="^refactor" --grep="^perf" \
        --format="%s" 2>/dev/null | head -20 || true
}

# Initialize milestones.json if missing
if [ ! -f "$MILESTONES_FILE" ]; then
    echo '{}' > "$MILESTONES_FILE"
fi

BASE=$(base_version "$VERSION")
NOW=$(now_iso)

case "$ACTION" in
    release)
        # Mark milestone as released
        EXISTING=$(cat "$MILESTONES_FILE")
        
        if echo "$EXISTING" | jq -e ".[\"$BASE\"]" > /dev/null 2>&1; then
            UPDATED=$(echo "$EXISTING" | jq \
                --arg base "$BASE" \
                --arg now "$NOW" \
                '.[$base].status = "released"
                | .[$base].frozen = true
                | .[$base].released = $now')
        else
            # Milestone didn't exist — create it as released
            UPDATED=$(echo "$EXISTING" | jq \
                --arg base "$BASE" \
                --arg now "$NOW" \
                '.[$base] = {
                    "status": "released",
                    "channel": "stable",
                    "nodes": [],
                    "frozen": true,
                    "opened": $now,
                    "released": $now,
                    "notes": []
                }')
        fi
        
        echo "$UPDATED" | jq '.' > "$MILESTONES_FILE"
        echo "  milestone: $BASE → released"
        ;;
        
    open)
        # Open next milestone (called after stable release)
        EXISTING=$(cat "$MILESTONES_FILE")
        
        if echo "$EXISTING" | jq -e ".[\"$BASE\"]" > /dev/null 2>&1; then
            echo "  milestone: $BASE already exists, skipping open"
        else
            UPDATED=$(echo "$EXISTING" | jq \
                --arg base "$BASE" \
                --arg now "$NOW" \
                '.[$base] = {
                    "status": "open",
                    "channel": "stable",
                    "nodes": [],
                    "frozen": false,
                    "opened": $now,
                    "released": null,
                    "notes": []
                }')
            echo "$UPDATED" | jq '.' > "$MILESTONES_FILE"
            echo "  milestone: $BASE → open"
        fi
        ;;
        
    nightly)
        # Record nightly build — separate channel, no RC semantics
        EXISTING=$(cat "$MILESTONES_FILE")
        
        # Nightlies use a special "nightly" key
        UPDATED=$(echo "$EXISTING" | jq \
            --arg ver "$VERSION" \
            --arg now "$NOW" \
            '.nightly = {
                "status": "nightly",
                "channel": "nightly",
                "version": $ver,
                "last_build": $now,
                "build_count": ((.nightly.build_count // 0) + 1)
            }')
        
        echo "$UPDATED" | jq '.' > "$MILESTONES_FILE"
        echo "  milestone: nightly → $VERSION"
        ;;
        
    *)
        echo "Unknown action: $ACTION"
        echo "Usage: milestone-update.sh <release|open|nightly> <version>"
        exit 1
        ;;
esac
