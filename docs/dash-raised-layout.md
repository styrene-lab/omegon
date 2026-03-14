---
id: dash-raised-layout
title: "Raised Dashboard: Horizontal Split Layout"
status: implemented
related: [dashboard-cleanup]
tags: [dashboard, tui, layout]
open_questions: []
openspec_change: dash-raised-layout
---

# Raised Dashboard: Horizontal Split Layout

## Overview

The raised /dash view currently stacks all sections (Design Tree → OpenSpec → Recovery → Cleave → Meta → Footer) vertically, consuming 10 lines while only filling ~half the terminal width. Terminals are typically much wider than tall. A proper left/right column split would use available horizontal space and show more useful content at once.

Key observation: `renderRaisedColumns()` already exists in footer.ts but is never called from `renderRaised()` — it's dead code. That method is a start but needs refinement.

## Research

### Current State Audit

- `renderRaised()` stacks: design tree → openspec → recovery → cleave → meta line → memory audit → separator rule → "/dash to compact" → footer data. Capped at 10 lines.
- `renderRaisedColumns()` exists but is dead code — never called. It does a basic 50/50 split: left=(design+recovery+cleave), right=(openspec). Uses `Math.floor((width - gutter) / 2)` with a 2-char gutter.
- Width threshold for "wide" in compact mode is ≥120 cols. No such threshold applied in raised mode.
- The screenshot shows the dashboard occupying only ~half the terminal width even though it's 2704px wide (Retina 2x). Actual terminal columns visible: probably 160-200+.
- The 10-line cap means we burn most of those lines on section headers and minimal content before hitting the limit.

### Layout Options

**A. Wire up existing renderRaisedColumns() (minimal)**
Simply call `renderRaisedColumns()` instead of `renderRaised()` when width ≥ 100 (or some threshold). Quick fix but the existing column method isn't great — no visual separator, padding logic strips ANSI correctly but the colWidth calc is rough.

**B. Proper 3-zone layout: left | divider | right**
Left col (~40-45% width): Design Tree nodes + Cleave status
Right col (~55-60% width): OpenSpec changes (typically has more content per item)
Thin ASCII vertical divider (`│`) between columns.
Footer meta line (context gauge, model, thinking) spans full width at bottom.
The "/dash to compact" + git/branch line spans full width below.

**C. Dynamic column assignment based on content**
Measure content lengths, assign whichever has more items to the wider column. Overkill for now.

**D. Raise the 10-line cap for wide terminals**
Current 10-line cap was set when the dashboard was content-poor. At 160+ cols with a 2-column layout we could use 12-14 lines and still leave ample editor space. Make the cap a function of terminal width.

**Recommended: B + partial D** — proper 3-zone with vertical divider, cap at 12 for wide terminals.

### Branch data gap analysis

To render the enriched git line we need branch data from producers. Current gaps:

**Design tree** (`dashboard-state.ts`):
- `implementingNodes[]` → only carries `branches?.[0]` (first branch)
- `nodes[]` → NO branch field at all (only id, title, status, questionCount, filePath)
- Need: expose `branches: string[]` on all active nodes in the `nodes[]` array

**OpenSpec** (extensions/openspec/index.ts):
- `OpenSpecChangeEntry` has no branch field
- OpenSpec changes don't inherently own a branch — they're directory-based, not branch-based
- Best approach: scan `git branch --list` at emit time for branches whose name contains the change name, or check if a `feature/<change-name>` branch exists. Simpler: leave OpenSpec branch-free for now; the design-tree node that `implements` a change carries the branch already.

**Decision**: Enrich `nodes[]` in `DesignTreeDashboardState` to include `branches: string[]`. The git line will collect all unique branches from: (a) `focusedNode.branch`, (b) all `nodes[].branches`, deduplicate, and exclude `main`/`master`. OpenSpec branch tracking deferred — the design node covers it.

### Git branch line visual design

User wants the branch display to look like an actual git branch tree — not a flat list. Unicode box-drawing + TypeScript gives us the tools to do this properly.

**Proposed: 1-or-2 line branch tree**

Zero branches:
```
⌂ ~/workspace/ai/Omegon  main
```

One branch:
```
⌂ ~/workspace/ai/Omegon  main ─── feature/dash-raised-layout
```

Two+ branches (fork shape — line 2 indented to align with fork point):
```
⌂ ~/workspace/ai/Omegon  main ─┬─ feature/dash-raised-layout
                                └─ feature/skill-aware-dispatch
```

Three+ branches (middle rows use ├, last uses └):
```
⌂ ~/workspace/ai/Omegon  main ─┬─ feature/dash-raised-layout
                                ├─ feature/skill-aware-dispatch  
                                └─ refactor/something-else
```

**Implementation notes:**
- Line 1: `prefix + repo_name + "  " + currentBranch + " ─┬─ " + branches[0]`
- Line 2+: `" ".repeat(indent) + "├─ " / "└─ " + branch`
- Indent = visibleWidth(prefix + repo_name + "  " + currentBranch + " ─")  
- Branch names are styled dim; current branch is `theme.fg("success", branch)` (matching footer convention); feature/* branches `accent`, fix/* `warning`
- Branches from design nodes get a `◈` suffix indicator; future: openspec `◎` suffix
- These 2 lines count toward the 12-line cap, replacing the current single pwd+branch line

### Git branch section — full tree visualization

Since the raised view is the expanded state, the git branch section is a first-class panel — not a compressed line. We have vertical room to render a real tree rooted at the repo name, with branches hanging off it like a proper git graph.

**Proposed structure:**

```
Omegon ─┬─ ● main
        ├─ feature/dash-raised-layout   ◈ Raised Dashboard: Horizontal Split Layout
        └─ feature/skill-aware-dispatch ◈ Skill-Aware Dispatch
```

Rules:
- Root node = repo name (basename of cwd, or `git rev-parse --show-toplevel` basename — already available)
- Current branch gets `●` prefix, colored `success`; non-current branches get `○` or no icon
- `main`/`master` always listed first after root
- Other branches sorted: feature/* then refactor/* then fix/* then rest
- Each branch that matches a design node or openspec change shows its title inline (right-aligned or space-separated) with the appropriate icon (`◈` design, `◎` openspec)
- Branch color conventions: main/master=success, feature/=accent, fix/hotfix/=warning, refactor/=accent(dim), others=muted
- Branch names are truncated to fit column width; titles truncated with `…` if needed
- This section lives in the **left column** of the 2-column layout, below Design Tree content, replacing the flat git line in the footer meta row

**Data requirements:**
- Current branch: `footerData.getGitBranch()` ✓
- All local branches: need a new mechanism — options:
  a. Read `.git/refs/heads/` directory (sync, no shell) 
  b. `execFileSync('git', ['branch', '--format=%(refname:short)'])` at emit time
  c. Add to file-watch system, re-emit on branch change
- Option (a) is simplest and zero-shell: `fs.readdirSync('.git/refs/heads')` + recurse for `refs/heads/feature/` subdirs. Already done in tree.ts (`readCurrentBranch` reads `.git/HEAD`).
- Branch → design node mapping: `matchBranchToNode()` already exists in `tree.ts` and does segment-aware matching. Expose it via dashboard state or call it at render time from the branch list.
- Branch → openspec change mapping: check if `openspec/changes/<branch-segment>/` exists for any segment of the branch name.

**Line budget in left column (12-line cap, ≥120):**
- Design Tree header + 2-3 nodes: ~4 lines
- Cleave status: 1-2 lines
- Git tree section: 3-4 lines (repo root + 2-3 branches)
- Total: ~9-10 lines left column content before shared footer zone

### Root cause of column misalignment bug

The existing `renderRaisedColumns()` computes column padding with:
```ts
const leftVisLen = left.replace(/\x1b\[[0-9;]*m/g, "").length;
const leftPad = Math.max(0, colWidth - leftVisLen);
```
This regex only strips SGR color codes (`ESC[...m`). It misses OSC 8 hyperlink sequences (`ESC]8;;url\x07text\x1b\\`) emitted by `linkDashboardFile()` and `linkOpenSpecChange()`. Those sequences inflate the raw string length, causing the padding to undercount, blowing up column alignment.

`visibleWidth()` from `@mariozechner/pi-tui` correctly handles OSC 8 (confirmed at utils.js:148). **All width measurements in the new implementation must use `visibleWidth()` — no raw `.length`, no hand-rolled regexes.**

The new implementation will use:
```ts
const leftPad = Math.max(0, colWidth - visibleWidth(left));
const line = left + " ".repeat(leftPad) + "│" + right;
// truncateToWidth handles the final width clamp
```

### Rendering primitive consolidation

Three broken measurement sites in footer.ts all share the same root cause: no shared primitive for common layout operations, so callers reach for raw `.replace(/\x1b\[[0-9;]*m/g, "").length` which misses OSC 8 hyperlinks.

`visibleWidth()` and `truncateToWidth()` are already imported and used correctly in `joinPrioritySegments`/`composePrimaryMetaLine`. What's missing is one level up:

**New module: `extensions/dashboard/render-utils.ts`**

```ts
// Pad a styled string to exactly `width` visible columns
export function padRight(s: string, width: number): string {
  const pad = Math.max(0, width - visibleWidth(s));
  return s + " ".repeat(pad);
}

// Place left flush-left, right flush-right within width
// Falls back to left-only if they don't fit together
export function leftRight(left: string, right: string, width: number): string {
  const lw = visibleWidth(left);
  const rw = visibleWidth(right);
  if (lw + rw > width) return truncateToWidth(left, width, "…");
  return left + " ".repeat(width - lw - rw) + right;
}

// Merge two column arrays side by side with a divider character
// leftWidth + 1 (divider) + rightWidth === total width
export function mergeColumns(
  leftLines: string[], rightLines: string[],
  leftWidth: number, rightWidth: number,
  divider = "│",
): string[] {
  const count = Math.max(leftLines.length, rightLines.length);
  return Array.from({ length: count }, (_, i) => {
    const l = i < leftLines.length ? truncateToWidth(leftLines[i], leftWidth, "…") : "";
    const r = i < rightLines.length ? truncateToWidth(rightLines[i], rightWidth, "…") : "";
    return padRight(l, leftWidth) + divider + r;
  });
}
```

`footer.ts` then replaces:
- Line 389-391 (`renderRaisedColumns`): deleted with the dead code
- Lines 825-826 (`renderFooterData` stats line): `leftRight(statsLeft, rightSide, width)`

All new column layout code in `renderRaisedWide` uses `mergeColumns` + `padRight` exclusively.

Tests in `render-utils.test.ts` cover:
- `padRight` with plain text, ANSI color, and OSC 8 hyperlinks
- `leftRight` with OSC 8 on both sides — assert `visibleWidth(result) === width`
- `mergeColumns` with mismatched row counts — assert every row is exactly `leftWidth + 1 + rightWidth` visible columns

### Pinned footer metadata problem

In raised mode the dashboard can grow enough that the footer metadata block (driver/model, thinking level, memory audit, and footer data) scrolls off the visible bottom. The current raised renderer appends those lines after the content sections, so they are not structurally pinned. A better layout is to keep the dashboard content flexible while reserving the final visible rows for the meta/footer block, with compact mode remaining a single dashboard-first line.

## Decisions

### Decision: Column split threshold: ≥120 columns

**Status:** decided
**Rationale:** Follow the existing ≥120 convention already used for "wide" breakpoint in compact mode. No new magic numbers.

### Decision: Footer zone spans full width below both columns

**Status:** decided
**Rationale:** Footer zone is 2 rows: (1) context gauge · model · thinking level indicator; (2) git branch line showing current branch + all branches from active design nodes + openspec changes. The git line gives at-a-glance "N branches open" visibility without entering another view.

### Decision: Line cap: 12 for wide (≥120), 10 for narrow

**Status:** exploring
**Rationale:** Initially proposed; superseded by "Remove the line cap in raised mode" once it was clear raised mode is an intentionally expanded view with no reason to artificially constrain output.

### Decision: Remove the line cap in raised mode

**Status:** decided
**Rationale:** Raised mode is an intentionally expanded view — the user toggled it to see detail. Capping at 10 or 12 lines was a holdover from when the dashboard was compact-first. In raised mode, render as many lines as the content warrants. The pi footer widget system scrolls naturally and the terminal handles overflow. Compact mode still stays at 1 line.

### Decision: Raised dashboard should pin model/thinking/memory metadata to the bottom

**Status:** exploring
**Rationale:** The operator relies on driver/model/thinking level and memory visibility as persistent session controls. In the raised dashboard, those values should remain anchored in the bottom rows while the higher-level dashboard sections above them absorb truncation or scrolling pressure.

## Open Questions

*No open questions.*

## Implementation Notes

### File Scope

- `extensions/dashboard/types.ts` (modified) — Add `branches?: string[]` to the nodes[] item shape in DesignTreeDashboardState
- `extensions/design-tree/dashboard-state.ts` (modified) — Populate branches[] on each node in the nodes[] array from n.branches
- `extensions/dashboard/footer.ts` (modified) — Replace renderRaised() with 3-zone layout: at ≥120 use renderRaisedColumns() (rewritten), else stacked. Delete dead renderRaisedColumns(). Add renderSharedFooterZone() for context gauge + enriched git line. Line cap: 12 wide / 10 narrow.
- `extensions/dashboard/footer-raised.test.ts` (modified) — Update/add tests for wide 2-column layout and enriched git branch line

### Constraints

- Use visibleWidth() from @mariozechner/pi-tui for all column padding — never .length on ANSI strings
- Width passed to render() is the real terminal width — use it directly, do not hard-code terminal assumptions
- Column split: leftWidth = Math.floor((width - 1) / 2), rightWidth = width - leftWidth - 1 (divider col), so columns are deterministic
- Vertical divider: a single │ character column between left and right
- Git line must deduplicate branches and exclude main/master/HEAD
- renderRaisedColumns() dead code must be deleted, not kept alongside the new implementation
