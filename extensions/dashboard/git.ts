/**
 * Git utilities for the dashboard extension.
 *
 * Reads local branches from .git/refs/heads/ without shell spawning,
 * and renders a unicode branch tree for the raised layout.
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { visibleWidth } from "@mariozechner/pi-tui";
import type { Theme } from "@mariozechner/pi-coding-agent";

// ── Branch reader ──────────────────────────────────────────────────────────────

/**
 * Recursively collect branch names from a directory, returning
 * slash-joined paths relative to the base directory.
 */
function collectRefs(dir: string, base: string): string[] {
  let results: string[] = [];
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const sub = collectRefs(fullPath, base);
      results = results.concat(sub);
    } else if (entry.isFile()) {
      const rel = path.relative(base, fullPath).split(path.sep).join("/");
      // Exclude HEAD and any name with illegal ref chars
      // Exclude HEAD and any name with illegal ref chars (spaces, control chars, ~^:?*\[)
      if (rel !== "HEAD" && !/[\x00-\x20\x7f ~^:?*[\\]/.test(rel)) {
        results.push(rel);
      }
    }
  }
  return results;
}

/**
 * Sort priority for branch names.
 * Lower number = earlier in list.
 */
function branchPriority(b: string): number {
  if (b === "main" || b === "master") return 0;
  if (b.startsWith("feature/")) return 1;
  if (b.startsWith("refactor/")) return 2;
  if (b.startsWith("fix/") || b.startsWith("hotfix/")) return 3;
  return 4;
}

/**
 * Read local branches from .git/refs/heads/ without spawning a shell.
 *
 * Returns branch names sorted: main/master first, then feature/*, refactor/*,
 * fix/hotfix, then the rest alphabetically.
 * Returns [] gracefully if the directory does not exist (detached HEAD, worktree, etc.).
 */
export function readLocalBranches(cwd: string): string[] {
  const headsDir = path.join(cwd, ".git", "refs", "heads");
  const branches = collectRefs(headsDir, headsDir);
  branches.sort((a, b) => {
    const pa = branchPriority(a);
    const pb = branchPriority(b);
    if (pa !== pb) return pa - pb;
    return a.localeCompare(b);
  });
  return branches;
}

// ── Branch tree renderer ───────────────────────────────────────────────────────

export interface BranchTreeParams {
  repoName: string;
  currentBranch: string | null;
  allBranches: string[];
  designNodes?: Array<{ branches?: string[]; title: string }>;
}

/**
 * Style a branch name according to its type and whether it is current.
 */
function styledBranch(b: string, isCurrent: boolean, theme: Theme): string {
  const label = isCurrent ? "● " + b : b;
  if (isCurrent) return theme.fg("success", label);
  if (b.startsWith("feature/")) return theme.fg("accent", b);
  if (b.startsWith("fix/") || b.startsWith("hotfix/")) return theme.fg("warning", b);
  if (b.startsWith("refactor/")) return theme.fg("accent", b); // dim accent via same color
  return theme.fg("muted", b);
}

/**
 * Find annotation for a branch from design nodes.
 */
function branchAnnotation(
  b: string,
  designNodes: Array<{ branches?: string[]; title: string }> | undefined,
  theme: Theme
): string {
  if (!designNodes) return "";
  const node = designNodes.find((n) => n.branches?.includes(b));
  if (!node) return "";
  return "  " + theme.fg("dim", "◈ " + node.title);
}

/**
 * Build the branch tree lines for the raised layout.
 *
 * - 0 branches: [dim(repoName)]
 * - 1 branch:   repoName + " ─── " + styledBranch
 * - N branches: repoName + " ─┬─ " + styledBranch(branches[0])
 *               indent       + "├─ " + styledBranch(branches[i])   (middle)
 *               indent       + "└─ " + styledBranch(branches[N-1]) (last)
 *
 * Current branch is placed first; deduplication ensures it appears only once.
 */
export function buildBranchTreeLines(params: BranchTreeParams, theme: Theme): string[] {
  const { repoName, currentBranch, allBranches, designNodes } = params;

  // Build ordered, deduplicated branch list: current first
  const ordered: string[] = [];
  if (currentBranch) {
    ordered.push(currentBranch);
  }
  for (const b of allBranches) {
    if (!ordered.includes(b)) {
      ordered.push(b);
    }
  }

  if (ordered.length === 0) {
    return [theme.fg("dim", repoName)];
  }

  if (ordered.length === 1) {
    const b = ordered[0];
    const isCurrent = b === currentBranch;
    const annotation = branchAnnotation(b, designNodes, theme);
    return [repoName + " ─── " + styledBranch(b, isCurrent, theme) + annotation];
  }

  // Multiple branches
  // indent = spaces equal to visibleWidth of "repoName + " ─""
  const indentWidth = visibleWidth(repoName + " ─");
  const indent = " ".repeat(indentWidth);

  const lines: string[] = [];
  for (let i = 0; i < ordered.length; i++) {
    const b = ordered[i];
    const isCurrent = b === currentBranch;
    const styled = styledBranch(b, isCurrent, theme);
    const annotation = branchAnnotation(b, designNodes, theme);

    if (i === 0) {
      lines.push(repoName + " ─┬─ " + styled + annotation);
    } else if (i < ordered.length - 1) {
      lines.push(indent + "├─ " + styled + annotation);
    } else {
      lines.push(indent + "└─ " + styled + annotation);
    }
  }

  return lines;
}
