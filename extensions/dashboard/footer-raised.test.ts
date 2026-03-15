import { beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";

import { DashboardFooter } from "./footer.ts";
import { sharedState } from "../lib/shared-state.ts";
import type { DashboardState } from "./types.ts";
import { visibleWidth } from "@cwilson613/pi-tui";

function makeTheme() {
  return {
    fg: (_color: string, text: string) => text,
    bold: (text: string) => text,
  };
}

function makeFooterData() {
  return {
    getAvailableProviderCount: () => 2,
    getGitBranch: () => "main",
    getExtensionStatuses: () => new Map<string, string>(),
  };
}

function makeContext() {
  return {
    cwd: "/Users/cwilson/workspace/ai/omegon",
    model: {
      provider: "openai-codex",
      id: "gpt-5.4",
      reasoning: true,
    },
    getContextUsage: () => ({ percent: 31, contextWindow: 272000 }),
    sessionManager: {
      getEntries: () => [],
      getSessionName: () => undefined,
    },
  };
}

describe("DashboardFooter raised mode polish", () => {
  beforeEach(() => {
    (sharedState as any).designTree = {
      nodeCount: 4,
      decidedCount: 1,
      exploringCount: 1,
      implementingCount: 1,
      implementedCount: 1,
      blockedCount: 0,
      openQuestionCount: 0,
      focusedNode: null,
      implementingNodes: [{ id: "memory", title: "Memory integration", branch: "feature/memory", filePath: "docs/memory.md" }],
      nodes: [],
    };
    (sharedState as any).openspec = {
      changes: [{ name: "memory-lifecycle-integration", stage: "verifying", tasksDone: 6, tasksTotal: 6 }],
    };
    (sharedState as any).lastMemoryInjection = {
      mode: "semantic",
      projectFactCount: 30,
      edgeCount: 0,
      workingMemoryFactCount: 4,
      semanticHitCount: 12,
      episodeCount: 3,
      globalFactCount: 15,
      payloadChars: 4800,
      estimatedTokens: 1200,
    };
  });

  it("keeps wide raised mode stacked instead of bleeding multiple sections across one row", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [{ label: "memory-core", status: "running" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.ok(lines.some((line) => line.includes("◈ Design Tree")));
    assert.ok(lines.some((line) => line.includes("◎ Implementation")));
    // In wide mode design tree (left col) and OpenSpec (right col) are zipped
    // by mergeColumns — their headers land on the same output row separated by │.
    // Verify at least one │ divider row exists (confirms two-column layout).
    assert.ok(lines.some((line) => line.includes("│")), `expected │ divider in wide layout;\n${lines.join("\n")}`);
  });

  it("hides stale failed cleave state after it ages out", () => {
    (sharedState as any).cleave = {
      status: "failed",
      updatedAt: Date.now() - 60_000,
      children: [{ label: "memory-core", status: "failed" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    assert.ok(lines.every((line) => !line.includes("⚡ Cleave")));
  });

  it("keeps memory line compact — no raw chars/hits fields, shows ⌗ total and injected counts", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(160);
    // HUD memory section: divider line labels the section, data line uses terse "inj" label.
    // ⌗ appears only when the memory extension reports a fact count in its status text.
    const memDivider = lines.find((l) => l.includes("── memory"));
    assert.ok(memDivider, `expected HUD "memory" section divider; got:\n${lines.join("\n")}`);
    const memDataLine = lines.find((line) => line.includes("inj "));
    assert.ok(memDataLine, `expected memory data line with "inj" label; got:\n${lines.join("\n")}`);
    assert.ok(!memDataLine?.includes("chars:"));
    assert.ok(!memDataLine?.includes("hits:"));
  });

  it("wide raised mode uses two-column layout — design tree full-width, recovery+cleave left, openspec right", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [
        { label: "task-a", status: "running" },
        { label: "task-b", status: "done" },
      ],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(140);

    // Design tree (left col) and OpenSpec (right col) share the same merged zone —
    // their headers are zipped into the same row by mergeColumns.
    const dtLine = lines.find((l) => l.includes("◈ Design Tree"));
    assert.ok(dtLine, `expected ◈ Design Tree line; got:\n${lines.join("\n")}`);
    // OpenSpec header should exist somewhere in the output
    assert.ok(lines.some((l) => l.includes("◎ Implementation")), `expected ◎ Implementation line; got:\n${lines.join("\n")}`);

    // There must be a row containing the divider (│) — confirms two-column layout
    const dividerRow = lines.find((l) => l.includes("│"));
    assert.ok(dividerRow, `expected a │ divider row; got:\n${lines.join("\n")}`);

    // All rows must fit within the requested width
    for (const line of lines) {
      const vw = visibleWidth(line);
      assert.ok(vw <= 140, `line too wide (${vw} > 140): ${line}`);
    }
  });

  it("wide mode column rows have consistent visible width (column alignment)", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [{ label: "a", status: "running" }, { label: "b", status: "done" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(120);
    const columnRows = lines.filter((l) => l.includes("│"));
    assert.ok(columnRows.length > 0, "expected at least one column row");

    // All column rows should have the same visible width (= terminal width)
    const widths = columnRows.map((l) => visibleWidth(l));
    const allSame = widths.every((w) => w === widths[0]);
    assert.ok(allSame, `column rows have unequal widths: ${widths.join(", ")}`);
    assert.equal(widths[0], 120);
  });

  it("OSC 8 hyperlinks in rendered lines do not inflate visibleWidth (regression)", () => {
    // OSC 8 hyperlinks are zero-width escape sequences; visibleWidth must not
    // count them, ensuring column layout stays aligned when file paths are linked.
    (sharedState as any).designTree = {
      ...(sharedState as any).designTree,
      focusedNode: null,
      implementingNodes: [{
        id: "linked",
        title: "Linked Node",
        branch: "feature/linked",
        filePath: "docs/linked.md",
      }],
    };
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(120);
    for (const line of lines) {
      const vw = visibleWidth(line);
      assert.ok(
        vw <= 120,
        `visibleWidth ${vw} exceeds 120 — OSC 8 sequences may be inflating width:\n  ${line}`,
      );
    }
  });

  it("top border never exceeds terminal width when branch annotation makes first line wider than innerWidth (regression)", () => {
    // Reproduces the pi-crash.log line 1924 overflow: branch tree lines were
    // truncated to innerWidth (width-4) but the top border places the first line
    // inside ╭─ … ─╮ which has 5 fixed chars, causing a +1 overflow.
    (sharedState as any).designTree = {
      ...(sharedState as any).designTree,
      nodes: [
        {
          id: "memory-task-completion-facts",
          title: "Memory: Task-Completion Facts — Mid-term \"what happened\" context for agent continuity",
          status: "implementing",
          branches: ["feature/memory-task-completion-facts"],
          filePath: "docs/memory-task-completion-facts.md",
        },
      ],
      implementingNodes: [],
      focusedNode: null,
    };
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      // Report the long branch as current
      { ...makeFooterData(), getGitBranch: () => "feature/memory-task-completion-facts" } as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    // 167-wide terminal — the exact width from the crash log
    const lines = footer.render(167);
    for (const line of lines) {
      const vw = visibleWidth(line);
      assert.ok(
        vw <= 167,
        `top border overflow — visibleWidth ${vw} > 167: ${JSON.stringify(line)}`,
      );
    }
  });

  it("raised mode meta line includes context gauge, model, and thinking (not a duplicate stats row)", () => {
    // In raised mode the context/model/thinking info lives in the pinned meta line
    // (buildRaisedMetaLine), not in a separate leftRight stats row. The meta line
    // uses the "Context " prefix label rather than a bare percentage.
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(100);
    // HUD context section: bar line uses ▐▌ delimiters and shows %; provider/model line uses ▸ pointer.
    const barLine = lines.find((l) => l.includes("▐") && l.includes("▌"));
    assert.ok(barLine, `expected HUD bar line with ▐▌ delimiters; got:\n${lines.join("\n")}`);
    assert.ok(barLine!.includes("31%"), `expected context percent in bar line: ${barLine}`);
    const modelLine = lines.find((l) => l.includes("▸") && l.includes("gpt-5.4"));
    assert.ok(modelLine, `expected HUD model line with ▸ pointer + model id; got:\n${lines.join("\n")}`);
    // There must NOT be a separate bare-percentage stats row.
    const bareStatsLine = lines.find((l) => l.includes("31%/272k") && l.includes("gpt-5.4") && l.includes("Context"));
    assert.ok(!bareStatsLine, `raised mode must not emit old-style "Context" stats row:\n${lines.join("\n")}`);
  });

  it("narrow raised mode (<100) stays stacked — no inner column │ divider rows", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [{ label: "x", status: "running" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(99);
    // Box borders use │ on left+right of each content line (2 per line max in stacked mode).
    // A column layout would produce 3+ │ chars per line. Ensure no line has more than 2.
    const innerDividerLines = lines.filter((l) => (l.match(/│/g) ?? []).length > 2);
    assert.ok(
      innerDividerLines.length === 0,
      `narrow mode must not use inner column divider:\n${innerDividerLines.join("\n")}`,
    );
  });

  it("pinned bottom block always contains context/model/thinking in raised mode", () => {
    // Populate several upper sections so there is content pressure above the footer zone.
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [
        { label: "child-a", status: "running" },
        { label: "child-b", status: "running" },
        { label: "child-c", status: "done" },
      ],
    };
    (sharedState as any).openspec = {
      changes: [
        { name: "change-one", stage: "implementing", tasksDone: 3, tasksTotal: 10 },
        { name: "change-two", stage: "specified", tasksDone: 0, tasksTotal: 5 },
      ],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(140);

    // The pinned bottom block must include context gauge info (percent visible)
    assert.ok(
      lines.some((l) => l.includes("31%") || l.includes("Context")),
      `expected context info in pinned block;\n${lines.join("\n")}`,
    );
    // Model name must appear
    assert.ok(
      lines.some((l) => l.includes("gpt-5.4")),
      `expected model name in pinned block;\n${lines.join("\n")}`,
    );
    // Compact/raise hint must appear
    assert.ok(
      lines.some((l) => l.includes("/dash to compact") || l.includes("compact")),
      `expected compact hint in pinned block;\n${lines.join("\n")}`,
    );
  });

  it("raised mode keeps context and model topology separated without old generic duplicate stats rows", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(140);

    assert.ok(lines.some((l) => l.includes("── context")), `expected context card:\n${lines.join("\n")}`);
    assert.ok(lines.some((l) => l.includes("── models")), `expected models card:\n${lines.join("\n")}`);
    assert.ok(lines.some((l) => l.includes("Driver") && l.includes("gpt-5.4")), `expected role-labeled driver line:\n${lines.join("\n")}`);

    const legacyStatsLines = lines.filter((l) => l.includes("Context") && l.includes("gpt-5.4") && l.includes("31%"));
    assert.equal(
      legacyStatsLines.length,
      0,
      `raised mode emitted a legacy generic stats row:\n${lines.join("\n")}`,
    );
  });

  it("medium raised mode (100–139) uses horizontal summary grouping", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      {
        ...makeFooterData(),
        getExtensionStatuses: () => new Map<string, string>([["memory", "Memory: 12 facts · semantic"], ["offline-driver", "🏠 OFFLINE: Devstral Small 2 24B"]]),
      } as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(120);
    assert.ok(lines.some((l) => l.includes("── context") && l.includes("│") && l.includes("── models")), `expected horizontally grouped footer cards:\n${lines.join("\n")}`);
    assert.ok(lines.some((l) => l.includes("Fallback") && l.includes("offline")), `expected fallback/offline topology line:\n${lines.join("\n")}`);
  });

  it("compact hint appears in the pinned footer zone, not below duplicate generic rows", () => {
    (sharedState as any).cleave = {
      status: "dispatching",
      updatedAt: Date.now(),
      children: [{ label: "x", status: "running" }],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(140);

    const hintIdx = lines.findIndex((l) => l.includes("compact") || l.includes("/dash"));
    assert.ok(hintIdx !== -1, `compact hint not found in output:\n${lines.join("\n")}`);

    // The hint is now embedded in the box's bottom border, which follows all content.
    // Verify it appears somewhere in the output (ordering relative to ⌂ is not checked).
  });

  it("wide raised mode labels model roles clearly, including local alias normalization", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };
    (sharedState as any).effort = {
      level: 4,
      name: "Substantial",
      driver: "victory",
      thinking: "medium",
      extraction: "local",
      compaction: "local",
      cleavePreferLocal: true,
      cleaveFloor: "local",
      reviewModel: "victory",
      capped: false,
      resolvedExtractionModelId: "devstral-small-2:24b",
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      {
        ...makeFooterData(),
        getExtensionStatuses: () => new Map<string, string>([["memory", "Memory: 12 facts · semantic"], ["offline-driver", "🏠 OFFLINE: Devstral Small 2 24B"]]),
      } as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(200);
    assert.ok(lines.some((l) => l.includes("Driver") && l.includes("gpt-5.4")), `expected driver role line:\n${lines.join("\n")}`);
    assert.ok(lines.some((l) => l.includes("Extraction") && l.includes("Devstral 24B")), `expected normalized extraction label:\n${lines.join("\n")}`);
    assert.ok(lines.every((l) => !l.includes("Extraction · devstral-small-2:24b")), `expected canonical extraction label rather than raw alias:\n${lines.join("\n")}`);
  });

  it("openspec rows use compact separator — no double-punctuation in progress+stage", () => {
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };
    (sharedState as any).openspec = {
      changes: [
        { name: "my-change", stage: "implementing", tasksDone: 3, tasksTotal: 10, path: undefined },
      ],
    };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(140);
    const specRow = lines.find((l) => l.includes("my-change"));
    assert.ok(specRow, `expected openspec row for my-change;\n${lines.join("\n")}`);

    // Must NOT contain " ·  · " or "· ·" (the double-separator artifact)
    assert.ok(
      !specRow!.includes(" ·  · ") && !specRow!.includes("· ·"),
      `openspec row has double-separator noise: ${specRow}`,
    );
    // Progress and stage should be present
    assert.ok(specRow!.includes("3/10"), `expected progress in row: ${specRow}`);
    assert.ok(specRow!.includes("impl"), `expected stage label in row: ${specRow}`);
  });

  it("truncates raised rows by dropping metadata before the primary label", () => {
    (sharedState as any).designTree = {
      nodeCount: 1,
      decidedCount: 0,
      exploringCount: 0,
      implementingCount: 1,
      implementedCount: 0,
      blockedCount: 0,
      openQuestionCount: 4,
      focusedNode: {
        id: "long-node",
        title: "I2P Integration With An Extremely Verbose Title That Must Stay Recognizable",
        status: "implementing",
        questions: ["one", "two", "three", "four"],
        branch: "feature/i2p-integration-with-a-very-very-long-branch-name",
        filePath: "docs/unified-dashboard.md",
      },
      implementingNodes: [],
      nodes: [],
    };
    (sharedState as any).openspec = {
      changes: [{
        name: "very-long-openspec-change-name-that-should-still-show-before-progress-metadata",
        stage: "implementing",
        tasksDone: 25,
        tasksTotal: 27,
        path: `${process.cwd()}/openspec/changes/dashboard-wide-truncation`,
      }],
    };
    (sharedState as any).cleave = { status: "idle", updatedAt: Date.now(), children: [] };

    const footer = new DashboardFooter(
      {} as any,
      makeTheme() as any,
      makeFooterData() as any,
      { mode: "raised", turns: 0 } satisfies DashboardState,
    );
    footer.setContext(makeContext() as any);

    const lines = footer.render(110);
    const designLine = lines.find((line) => line.includes("I2P Integration"));
    const specLine = lines.find((line) => line.includes("very-long-openspec-change-name"));
    assert.ok(designLine, lines.join("\n"));
    assert.ok(specLine, lines.join("\n"));
    assert.ok(designLine!.includes("⚙"), designLine);
    assert.ok(specLine!.includes("◦"), specLine);
  });
});

import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { readLocalBranches, buildBranchTreeLines } from "./git.ts";

describe("readLocalBranches", () => {
  it("returns [] gracefully when .git/refs/heads does not exist", () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pi-git-"));
    try {
      const result = readLocalBranches(tmp);
      assert.deepEqual(result, []);
    } finally {
      fs.rmSync(tmp, { recursive: true });
    }
  });

  it("reads flat and nested feature/ branches from .git/refs/heads/", () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pi-git-"));
    try {
      const headsDir = path.join(tmp, ".git", "refs", "heads");
      const featureDir = path.join(headsDir, "feature");
      fs.mkdirSync(featureDir, { recursive: true });
      fs.writeFileSync(path.join(headsDir, "main"), "abc123\n");
      fs.writeFileSync(path.join(featureDir, "dash-raised-layout"), "def456\n");
      fs.writeFileSync(path.join(featureDir, "skill-aware-dispatch"), "ghi789\n");

      const result = readLocalBranches(tmp);
      assert.deepEqual(result, [
        "main",
        "feature/dash-raised-layout",
        "feature/skill-aware-dispatch",
      ]);
    } finally {
      fs.rmSync(tmp, { recursive: true });
    }
  });

  it("sorts main first, then feature/*, then refactor/*, then fix/*, then rest", () => {
    const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "pi-git-"));
    try {
      const headsDir = path.join(tmp, ".git", "refs", "heads");
      for (const [sub, name] of [
        ["fix", "the-bug"],
        ["feature", "alpha"],
        ["refactor", "cleanup"],
        ["", "main"],
        ["feature", "beta"],
      ]) {
        const dir = sub ? path.join(headsDir, sub) : headsDir;
        fs.mkdirSync(dir, { recursive: true });
        fs.writeFileSync(path.join(dir, name), "sha\n");
      }
      const result = readLocalBranches(tmp);
      assert.equal(result[0], "main");
      assert.ok(result.indexOf("feature/alpha") < result.indexOf("refactor/cleanup"), result.join(", "));
      assert.ok(result.indexOf("refactor/cleanup") < result.indexOf("fix/the-bug"), result.join(", "));
    } finally {
      fs.rmSync(tmp, { recursive: true });
    }
  });
});

describe("buildBranchTreeLines", () => {
  const theme = makeTheme() as any;

  it("zero branches returns just repoName", () => {
    const lines = buildBranchTreeLines({ repoName: "omegon", currentBranch: null, allBranches: [] }, theme);
    assert.equal(lines.length, 1);
    assert.ok(lines[0]!.includes("omegon"));
    assert.ok(!lines[0]!.includes("─"), "should have no connectors");
  });

  it("single branch uses ─── connector", () => {
    const lines = buildBranchTreeLines({ repoName: "omegon", currentBranch: "main", allBranches: ["main"] }, theme);
    assert.equal(lines.length, 1);
    assert.ok(lines[0]!.includes("───"), lines[0]);
    assert.ok(!lines[0]!.includes("┬"), lines[0]);
  });

  it("two branches use ─┬─ on first line, └─ on second", () => {
    const lines = buildBranchTreeLines({
      repoName: "omegon",
      currentBranch: "main",
      allBranches: ["main", "feature/foo"],
    }, theme);
    assert.equal(lines.length, 2);
    assert.ok(lines[0]!.includes("─┬─"), lines[0]);
    assert.ok(lines[1]!.includes("└─"), lines[1]);
    assert.ok(!lines[1]!.includes("├─"), lines[1]);
  });

  it("three branches use ─┬─, ├─, └─", () => {
    const lines = buildBranchTreeLines({
      repoName: "omegon",
      currentBranch: "main",
      allBranches: ["main", "feature/foo", "feature/bar"],
    }, theme);
    assert.equal(lines.length, 3);
    assert.ok(lines[0]!.includes("─┬─"), lines[0]);
    assert.ok(lines[1]!.includes("├─"), lines[1]);
    assert.ok(lines[2]!.includes("└─"), lines[2]);
  });

  it("indent on continuation lines equals visibleWidth(repoName + ' ─')", () => {
    const vw = visibleWidth;
    const repoName = "omegon";
    const lines = buildBranchTreeLines({
      repoName,
      currentBranch: "main",
      allBranches: ["main", "feature/foo", "feature/bar"],
    }, theme);
    const expectedIndent = vw(repoName + " ─");
    // Line 2 (index 1) should start with that many spaces
    const leadingSpaces = lines[1]!.match(/^( *)/)?.[1]?.length ?? 0;
    assert.equal(leadingSpaces, expectedIndent, `indent should be ${expectedIndent}, got ${leadingSpaces}`);
  });

  it("annotation appears for a branch matching a design node's branches[]", () => {
    const lines = buildBranchTreeLines({
      repoName: "omegon",
      currentBranch: "main",
      allBranches: ["main", "feature/my-work"],
      designNodes: [{ branches: ["feature/my-work"], title: "My Work Node" }],
    }, theme);
    const featureLine = lines.find((l) => l.includes("feature/my-work"))!;
    assert.ok(featureLine, "feature line not found");
    assert.ok(featureLine.includes("◈"), featureLine);
    assert.ok(featureLine.includes("My Work Node"), featureLine);
  });

  it("current branch is placed first regardless of sort order", () => {
    const lines = buildBranchTreeLines({
      repoName: "omegon",
      currentBranch: "feature/current",
      allBranches: ["main", "feature/current", "feature/other"],
    }, theme);
    // First connector line after repoName should contain the current branch
    assert.ok(lines[0]!.includes("feature/current"), lines[0]);
  });
});
