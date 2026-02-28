/**
 * Project Memory Extension — v2 (SQLite-backed)
 *
 * Persistent, cross-session project knowledge stored in SQLite with
 * confidence-decay reinforcement. Facts that aren't encountered in
 * sessions gradually fade; facts that keep appearing grow more durable.
 *
 * Storage: .pi/memory/facts.db (SQLite with WAL mode)
 * Rendering: Active facts → Markdown-KV for LLM context injection
 *
 * Tools:
 *   memory_query          — Read active memory (rendered Markdown-KV)
 *   memory_store          — Explicitly add a fact
 *   memory_search_archive — Search all facts (including archived/superseded)
 *
 * Commands:
 *   /memory               — Interactive mind manager
 *   /memory edit           — Edit current mind in editor
 *   /memory refresh        — Re-evaluate and prune memory
 *   /memory clear          — Reset current mind
 *
 * Background extraction via subagent outputs JSONL actions.
 */

import * as path from "node:path";
import * as os from "node:os";
import type { ExtensionAPI, ExtensionContext, ExtensionCommandContext, SessionMessageEntry } from "@mariozechner/pi-coding-agent";
import { DynamicBorder } from "@mariozechner/pi-coding-agent";
import { StringEnum } from "@mariozechner/pi-ai";
import { Type } from "@sinclair/typebox";
import { Container, type SelectItem, SelectList, Text } from "@mariozechner/pi-tui";
import { FactStore, parseExtractionOutput, GLOBAL_DECAY, type MindRecord } from "./factstore.js";
import { DEFAULT_CONFIG, type MemoryConfig } from "./types.js";
import {
  type ExtractionTriggerState,
  createTriggerState,
  shouldExtract,
} from "./triggers.js";
import { runExtractionV2, runGlobalExtraction } from "./extraction-v2.js";
import { migrateToFactStore, needsMigration, markMigrated } from "./migration.js";
import { SECTIONS } from "./template.js";
import { serializeConversation, convertToLlm } from "@mariozechner/pi-coding-agent";

const VALID_MIND_NAME = /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/;

function sanitizeMindName(input: string): string | null {
  const sanitized = input.trim().replace(/[^a-zA-Z0-9_-]/g, "-").replace(/^[^a-zA-Z0-9]+/, "");
  if (!sanitized || !VALID_MIND_NAME.test(sanitized)) return null;
  return sanitized;
}

export default function (pi: ExtensionAPI) {
  let store: FactStore | null = null;
  let globalStore: FactStore | null = null;
  let triggerState: ExtractionTriggerState = createTriggerState();
  let postCompaction = false;
  let firstTurn = true;
  let config: MemoryConfig = { ...DEFAULT_CONFIG };
  let activeExtractionPromise: Promise<void> | null = null;
  let sessionActive = false;
  let consecutiveExtractionFailures = 0;
  let memoryDir = "";
  const globalMemoryDir = path.join(os.homedir(), ".pi", "memory");

  /** Get the active mind name (null = default) */
  function activeMind(): string {
    return store?.getActiveMind() ?? "default";
  }

  function activeLabel(): string {
    const mind = store?.getActiveMind();
    return mind ?? "default";
  }

  // --- Lifecycle ---

  pi.on("session_start", async (_event, ctx) => {
    memoryDir = path.join(ctx.cwd, ".pi", "memory");

    // Check for migration
    if (needsMigration(memoryDir)) {
      store = new FactStore(memoryDir);
      const result = migrateToFactStore(memoryDir, store);
      markMigrated(memoryDir);
      if (ctx.hasUI) {
        const msg = `Memory migrated to SQLite: ${result.factsImported} facts imported, ${result.archiveFactsImported} archive facts, ${result.mindsImported} minds`;
        ctx.ui.notify(msg, "success");
      }
    } else {
      store = new FactStore(memoryDir);
    }

    // Initialize global store (user-level, shared across projects)
    // Skip if project memory IS the global path (e.g., working from ~/)
    if (path.resolve(memoryDir) !== path.resolve(globalMemoryDir)) {
      globalStore = new FactStore(globalMemoryDir, { decay: GLOBAL_DECAY });
    }

    // Ensure .gitignore covers memory/
    const gitignorePath = path.join(memoryDir, "..", ".gitignore");
    try {
      const fs = await import("node:fs");
      const existing = fs.existsSync(gitignorePath)
        ? fs.readFileSync(gitignorePath, "utf8")
        : "";
      if (!existing.includes("memory/")) {
        const entry = existing.endsWith("\n") || existing === "" ? "memory/\n" : "\nmemory/\n";
        fs.writeFileSync(gitignorePath, existing + entry, "utf8");
      }
    } catch {
      // Best effort
    }

    triggerState = createTriggerState();
    postCompaction = false;
    firstTurn = true;
    activeExtractionPromise = null;
    sessionActive = true;
    consecutiveExtractionFailures = 0;
    updateStatus(ctx);
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    sessionActive = false;

    // Only wait briefly for an already-running extraction.
    // Do NOT start a new extraction during shutdown — the TUI remains interactive
    // and long-running extractions cause stray keypresses to trigger UI overlays
    // (e.g., Session Tree). Unextracted data persists and will be picked up next session.
    if (activeExtractionPromise) {
      if (ctx.hasUI) {
        ctx.ui.setStatus("memory", ctx.ui.theme.fg("dim", "saving memory…"));
      }
      let timeoutId: NodeJS.Timeout | null = null;
      const timeout = new Promise<void>((resolve) => {
        timeoutId = setTimeout(resolve, 5_000);
      });
      await Promise.race([activeExtractionPromise, timeout]);
      if (timeoutId) clearTimeout(timeoutId);
    }

    store?.close(); globalStore?.close();
  });

  pi.on("session_compact", async (_event, ctx) => {
    postCompaction = true;

    if (store && !triggerState.isRunning) {
      triggerState.isRunning = true;
      try {
        await runExtractionCycle(ctx, config);
        const usage = ctx.getContextUsage();
        triggerState.lastExtractedTokens = usage?.tokens ?? 0;
        triggerState.isInitialized = true;
        consecutiveExtractionFailures = 0;
      } catch {
        consecutiveExtractionFailures++;
      } finally {
        triggerState.isRunning = false;
      }
    }

    triggerState.toolCallsSinceExtract = 0;
    triggerState.manualStoresSinceExtract = 0;
  });

  // --- Extraction cycle ---

  async function runExtractionCycle(ctx: ExtensionContext, cfg: MemoryConfig): Promise<void> {
    if (!store) return;

    const mind = activeMind();
    const currentFacts = store.getActiveFacts(mind);

    const branch = ctx.sessionManager.getBranch();
    const messages = branch
      .filter((e): e is SessionMessageEntry => e.type === "message")
      .map((e) => e.message);

    const recentMessages = messages.slice(-30);
    if (recentMessages.length === 0) return;

    const serialized = serializeConversation(convertToLlm(recentMessages));
    const rawOutput = await runExtractionV2(ctx.cwd, currentFacts, serialized, cfg);

    if (!rawOutput.trim()) return;

    const actions = parseExtractionOutput(rawOutput);
    if (actions.length > 0) {
      const result = store.processExtraction(mind, actions);

      // Phase 2: Global extraction — if new facts were created and global store exists
      if (result.newFactIds.length > 0 && globalStore) {
        try {
          const newFacts = result.newFactIds
            .map(id => store!.getFact(id))
            .filter((f): f is NonNullable<typeof f> => f !== null);

          if (newFacts.length > 0) {
            const globalMind = globalStore.getActiveMind() ?? "default";
            const globalFacts = globalStore.getActiveFacts(globalMind);
            const globalEdges = globalStore.getActiveEdges();

            const globalRawOutput = await runGlobalExtraction(
              ctx.cwd, newFacts, globalFacts, globalEdges, cfg,
            );

            if (globalRawOutput.trim()) {
              const globalActions = parseExtractionOutput(globalRawOutput);
              // Process fact actions first — observe creates new global facts
              // that connect actions can then reference
              const factActions = globalActions.filter(a => a.type !== "connect");
              const edgeActions = globalActions.filter(a => a.type === "connect");

              if (factActions.length > 0) {
                globalStore.processExtraction(globalMind, factActions);
              }
              // Edge actions reference global fact IDs (from existing global facts
              // or from facts just promoted via observe in the same extraction).
              // processEdges validates both endpoints exist before storing.
              if (edgeActions.length > 0) {
                globalStore.processEdges(edgeActions);
              }
            }
          }
        } catch (err) {
          // Global extraction is best-effort — don't fail the whole cycle
          if (ctx.hasUI) {
            ctx.ui.notify(`Global extraction failed: ${(err as Error).message}`, "warning");
          }
        }
      }
    }

    updateStatus(ctx);
  }

  /**
   * Run a memory refresh — extraction with no conversation context.
   * Used by /memory refresh command and __refresh__ menu action.
   */
  function startRefresh(ctx: ExtensionCommandContext): void {
    ctx.ui.notify("Running extraction to prune and consolidate memory (15–60s)…", "info");
    activeExtractionPromise = (async () => {
      try {
        triggerState.isRunning = true;
        const mind = activeMind();
        const currentFacts = store!.getActiveFacts(mind);
        const rawOutput = await runExtractionV2(
          ctx.cwd,
          currentFacts,
          `[Memory refresh requested. Review existing facts for accuracy and relevance. Archive stale or redundant facts. No new conversation context.]`,
          config,
        );
        if (rawOutput.trim()) {
          const actions = parseExtractionOutput(rawOutput);
          if (actions.length > 0) {
            const result = store!.processExtraction(mind, actions);
            ctx.ui.notify(
              `Memory refreshed: ${result.added} added, ${result.reinforced} reinforced`,
              "success",
            );
          }
        } else {
          ctx.ui.notify("No changes needed", "info");
        }
        updateStatus(ctx);
      } catch (err: any) {
        ctx.ui.notify(`Refresh failed: ${err.message}`, "error");
      } finally {
        triggerState.isRunning = false;
      }
    })();
    activeExtractionPromise.finally(() => { activeExtractionPromise = null; });
  }

  // --- Context Injection ---

  pi.on("before_agent_start", async (event, ctx) => {
    if (!store) return;
    if (!firstTurn && !postCompaction) return;

    firstTurn = false;
    postCompaction = false;

    const mind = activeMind();
    const factCount = store.countActiveFacts(mind);
    const mindLabel = mind !== "default" ? ` (mind: ${mind})` : "";

    if (factCount <= 3) {
      return {
        message: {
          customType: "project-memory",
          content: [
            `Project memory initialized${mindLabel} (${factCount} facts stored).`,
            "Use **memory_store** to persist important discoveries as you work",
            "(architecture decisions, constraints, patterns, known issues).",
            "Facts persist across sessions and will be available next time.",
          ].join(" "),
          display: false,
        },
      };
    }

    const rendered = store.renderForInjection(mind);

    // Include global knowledge if available
    let globalSection = "";
    if (globalStore) {
      const globalMind = globalStore.getActiveMind() ?? "default";
      const globalFactCount = globalStore.countActiveFacts(globalMind);
      if (globalFactCount > 0) {
        const globalRendered = globalStore.renderForInjection(globalMind, { maxFacts: 30 });
        globalSection = `\n\n<!-- Global Knowledge — cross-project facts and connections -->\n${globalRendered}`;
      }
    }

    return {
      message: {
        customType: "project-memory",
        content: [
          `Project memory available${mindLabel} (${factCount} facts from this and previous sessions).`,
          "Use **memory_query** to read accumulated knowledge about this project.",
          "Use **memory_store** to persist important discoveries (architecture decisions, constraints, patterns, known issues).",
          "Use **memory_search_archive** to search older archived facts.\n\n",
          rendered,
          globalSection,
        ].join(" "),
        display: false,
      },
    };
  });

  // --- Background Extraction Triggers ---

  pi.on("tool_execution_end", async (event, ctx) => {
    if (!store) return;

    triggerState.toolCallsSinceExtract++;

    if (event.toolName === "memory_store" && !event.isError) {
      triggerState.manualStoresSinceExtract++;
    }

    const usage = ctx.getContextUsage();
    if (!usage) return;

    if (shouldExtract(triggerState, usage.tokens, config, consecutiveExtractionFailures)) {
      activeExtractionPromise = (async () => {
        if (!store || triggerState.isRunning) return;
        triggerState.isRunning = true;
        try {
          await runExtractionCycle(ctx, config);
          const usage = ctx.getContextUsage();
          triggerState.lastExtractedTokens = usage?.tokens ?? 0;
          triggerState.toolCallsSinceExtract = 0;
          triggerState.manualStoresSinceExtract = 0;
          triggerState.isInitialized = true;
          consecutiveExtractionFailures = 0;
        } catch {
          consecutiveExtractionFailures++;
        } finally {
          triggerState.isRunning = false;
        }
      })();
      activeExtractionPromise.catch(() => {}).finally(() => { activeExtractionPromise = null; });
    }
  });

  // --- Tools ---

  pi.registerTool({
    name: "memory_query",
    label: "Project Memory",
    description: [
      "Read project memory — accumulated knowledge about this project's architecture,",
      "decisions, constraints, known issues, and patterns from this and previous sessions.",
      "Use when you need context about why something was done a certain way,",
      "known problems, or project conventions.",
    ].join(" "),
    parameters: Type.Object({}),
    async execute() {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }] };
      }
      const mind = activeMind();
      const rendered = store.renderForInjection(mind);
      const factCount = store.countActiveFacts(mind);
      return {
        content: [{ type: "text", text: rendered }],
        details: { facts: factCount, mind: activeLabel() },
      };
    },
  });

  pi.registerTool({
    name: "memory_store",
    label: "Store Memory",
    description: [
      "Explicitly add or update a fact in project memory.",
      "Use for important discoveries: architectural decisions, constraints,",
      "non-obvious patterns, tricky bugs, environment details.",
      "Facts persist across sessions.",
    ].join(" "),
    parameters: Type.Object({
      section: StringEnum(
        ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions"] as const,
        { description: "Memory section to add the fact to" },
      ),
      content: Type.String({
        description: "Fact to add (single bullet point, self-contained)",
      }),
    }),
    async execute(_toolCallId, params) {
      if (!store) {
        return {
          content: [{ type: "text", text: "Project memory not initialized." }],
          isError: true,
        };
      }

      const mind = activeMind();
      const content = params.content.replace(/^-\s*/, "").trim();
      const result = store.storeFact({
        mind,
        section: params.section as any,
        content,
        source: "manual",
      });

      if (result.duplicate) {
        return {
          content: [{ type: "text", text: `Reinforced existing fact in ${params.section}: ${content}` }],
          details: { section: params.section, reinforced: true, id: result.id },
        };
      }

      return {
        content: [{ type: "text", text: `Stored in ${params.section}: ${content}` }],
        details: { section: params.section, id: result.id, facts: store.countActiveFacts(mind) },
      };
    },
  });

  pi.registerTool({
    name: "memory_search_archive",
    label: "Search Memory Archive",
    description: [
      "Search archived project memories from previous months.",
      "Use when active memory doesn't have historical context you need —",
      "past decisions, old constraints, migration history, removed facts.",
    ].join(" "),
    parameters: Type.Object({
      query: Type.String({ description: "Search terms (file paths, symbol names, concepts)" }),
    }),
    async execute(_toolCallId, params) {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }] };
      }

      const mind = activeMind();
      const results = store.searchArchive(params.query, mind);

      if (results.length === 0) {
        // Also try cross-mind search
        const allResults = store.searchArchive(params.query);
        if (allResults.length === 0) {
          return { content: [{ type: "text", text: "No matches in memory archive." }] };
        }

        const formatted = allResults
          .map(f => `[${f.mind}/${f.section}] ${f.content} (${f.status}, ${f.created_at.split("T")[0]})`)
          .join("\n");

        return {
          content: [{ type: "text", text: `Cross-mind archive results:\n${formatted}` }],
          details: { totalMatches: allResults.length, crossMind: true },
        };
      }

      const formatted = results
        .map(f => `[${f.section}] ${f.content} (${f.status}, ${f.created_at.split("T")[0]})`)
        .join("\n");

      return {
        content: [{ type: "text", text: formatted }],
        details: { totalMatches: results.length },
      };
    },
  });

  pi.registerTool({
    name: "memory_connect",
    label: "Connect Facts",
    description: [
      "Create a directional relationship (edge) between two facts in the global knowledge base.",
      "Use when you identify meaningful connections between facts — dependencies, contradictions,",
      "generalizations, or causal relationships. Search for facts first to get their IDs.",
      "The relation is a short verb phrase describing the relationship from source to target.",
      "Common patterns: runs_on, depends_on, motivated_by, contradicts, enables, generalizes,",
      "instance_of, requires, conflicts_with, replaces, preceded_by.",
    ].join(" "),
    parameters: Type.Object({
      source_fact_id: Type.String({ description: "ID of the source fact" }),
      target_fact_id: Type.String({ description: "ID of the target fact" }),
      relation: Type.String({ description: "Short verb phrase: runs_on, depends_on, contradicts, etc." }),
      description: Type.String({ description: "Human-readable description of why these facts are connected" }),
    }),
    async execute(_toolCallId, params) {
      // Resolve which store owns each fact — edges must connect facts in the same DB
      const sourceInProject = store?.getFact(params.source_fact_id);
      const sourceInGlobal = globalStore?.getFact(params.source_fact_id);
      const targetInProject = store?.getFact(params.target_fact_id);
      const targetInGlobal = globalStore?.getFact(params.target_fact_id);

      const sourceFact = sourceInProject ?? sourceInGlobal;
      const targetFact = targetInProject ?? targetInGlobal;

      if (!sourceFact) {
        return { content: [{ type: "text", text: `Source fact not found: ${params.source_fact_id}` }], isError: true };
      }
      if (!targetFact) {
        return { content: [{ type: "text", text: `Target fact not found: ${params.target_fact_id}` }], isError: true };
      }

      // Both facts must be in the same store for FK integrity
      const sourceIsGlobal = !!sourceInGlobal && !sourceInProject;
      const targetIsGlobal = !!targetInGlobal && !targetInProject;

      let edgeStore: FactStore;
      if (sourceIsGlobal && targetIsGlobal) {
        edgeStore = globalStore!;
      } else if (!sourceIsGlobal && !targetIsGlobal) {
        edgeStore = store!;
      } else {
        return {
          content: [{ type: "text", text: "Cannot connect facts across databases. Both facts must be in the same store (project or global). Promote the project fact to global first." }],
          isError: true,
        };
      }

      const result = edgeStore.storeEdge({
        sourceFact: params.source_fact_id,
        targetFact: params.target_fact_id,
        relation: params.relation,
        description: params.description,
        sourceMind: sourceFact.mind,
        targetMind: targetFact.mind,
      });

      if (result.duplicate) {
        return {
          content: [{ type: "text", text: `Reinforced existing connection: ${sourceFact.content.slice(0, 50)} --${params.relation}--> ${targetFact.content.slice(0, 50)}` }],
          details: { id: result.id, reinforced: true },
        };
      }

      return {
        content: [{ type: "text", text: `Connected: ${sourceFact.content.slice(0, 50)} --${params.relation}--> ${targetFact.content.slice(0, 50)}` }],
        details: { id: result.id, source: params.source_fact_id, target: params.target_fact_id, relation: params.relation },
      };
    },
  });

  // --- Interactive Mind Manager ---

  function buildMindItems(minds: (MindRecord & { factCount: number })[], activeName: string): SelectItem[] {
    const items: SelectItem[] = [];

    for (const mind of minds) {
      const isActive = activeName === mind.name;
      const badges: string[] = [
        isActive ? "active target" : mind.status,
        `${mind.factCount} facts`,
      ];
      if (mind.readonly) badges.push("read-only");
      if (mind.origin_type === "link") badges.push("linked");
      if (mind.description) badges.push(mind.description);
      if (mind.parent) badges.push(`(from: ${mind.parent})`);
      items.push({
        value: mind.name,
        label: `${isActive ? "▸ " : "  "}${mind.name}`,
        description: badges.join(" • "),
      });
    }

    items.push({ value: "__create__", label: "  + Create new mind", description: "Start a fresh memory store" });
    items.push({ value: "__link__", label: "  ⟷ Link external mind", description: "Import from a path (read-only)" });
    items.push({ value: "__edit__", label: "  ✎ Edit current mind", description: "Open rendered view in editor" });
    items.push({ value: "__refresh__", label: "  ↻ Refresh current mind", description: "Run extraction to prune and consolidate" });

    return items;
  }

  function notifyMindSwitch(newLabel: string, factCount: number): void {
    pi.sendMessage({
      customType: "project-memory",
      content: [
        `Memory context switched to "${newLabel}" (${factCount} facts).`,
        "Your previous memory_query results are stale.",
        "Use **memory_query** to read the current memory if you need project context.",
      ].join(" "),
      display: false,
    }, {
      deliverAs: "nextTurn",
    });
  }

  async function showMindActions(ctx: ExtensionCommandContext, mindName: string): Promise<void> {
    if (!store) return;

    const mind = store.getMind(mindName);
    if (!mind) {
      ctx.ui.notify(`Mind "${mindName}" not found`, "error");
      return;
    }

    const isReadonly = mind.readonly === 1;
    const isLinked = mind.origin_type === "link";

    const actions: SelectItem[] = [
      { value: "switch", label: "Switch to this mind", description: "Make it the active memory store" },
    ];
    if (!isReadonly) {
      actions.push({ value: "edit", label: "Edit in editor", description: "Edit rendered memory as markdown" });
    }
    if (isLinked) {
      actions.push({ value: "sync", label: "Sync from source", description: `Pull latest from ${mind.origin_path}` });
    }
    actions.push({ value: "fork", label: "Fork", description: "Create a writable copy with a new name" });
    actions.push({ value: "ingest", label: "Ingest into another mind", description: "Merge facts into a target" });
    if (!isReadonly && mindName !== "default") {
      actions.push({ value: "status", label: "Change status", description: `Currently: ${mind.status}` });
    }
    if (mindName !== "default") {
      actions.push({ value: "delete", label: "Delete", description: isLinked ? "Remove link (source unaffected)" : "Remove this mind permanently" });
    }

    const action = await ctx.ui.custom<string | null>((tui, theme, _kb, done) => {
      const container = new Container();
      container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));
      container.addChild(new Text(theme.fg("accent", theme.bold(` Mind: ${mindName} `)), 1, 0));
      if (mind.description) {
        container.addChild(new Text(theme.fg("muted", ` ${mind.description}`), 1, 0));
      }

      const selectList = new SelectList(actions, Math.min(actions.length, 10), {
        selectedPrefix: (t) => theme.fg("accent", t),
        selectedText: (t) => theme.fg("accent", t),
        description: (t) => theme.fg("muted", t),
        scrollInfo: (t) => theme.fg("dim", t),
        noMatch: (t) => theme.fg("warning", t),
      });
      selectList.onSelect = (item) => done(item.value);
      selectList.onCancel = () => done(null);
      container.addChild(selectList);
      container.addChild(new Text(theme.fg("dim", " ↑↓ navigate • enter select • esc back"), 1, 0));
      container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));

      return {
        render: (w) => container.render(w),
        invalidate: () => container.invalidate(),
        handleInput: (data) => { selectList.handleInput(data); tui.requestRender(); },
      };
    });

    if (!action) return;

    switch (action) {
      case "switch": {
        store!.setActiveMind(mindName === "default" ? null : mindName);
        updateStatus(ctx);
        const count = store!.countActiveFacts(mindName);
        ctx.ui.notify(`Switched to mind: ${mindName}`, "success");
        notifyMindSwitch(mindName, count);
        break;
      }
      case "edit": {
        const rendered = store!.renderForInjection(mindName);
        const edited = await ctx.ui.editor(`Edit Mind: ${mindName}`, rendered);
        if (edited !== undefined && edited !== rendered) {
          // Parse edited markdown back into facts — this is lossy but useful
          // Archive all current facts and re-import from edited content
          ctx.ui.notify("Direct editing not yet supported for SQLite store. Use memory_store tool.", "warning");
        }
        break;
      }
      case "fork": {
        const rawName = await ctx.ui.input("New mind name:");
        if (!rawName?.trim()) return;
        const newName = sanitizeMindName(rawName);
        if (!newName) {
          ctx.ui.notify("Name must contain at least one alphanumeric character", "error");
          return;
        }
        if (newName !== rawName.trim()) {
          ctx.ui.notify(`Name sanitized to: ${newName}`, "info");
        }
        if (store!.mindExists(newName)) {
          ctx.ui.notify(`Mind "${newName}" already exists`, "error");
          return;
        }
        const desc = await ctx.ui.input("Description:", `Fork of ${mindName}`);
        store!.forkMind(mindName, newName, desc ?? `Fork of ${mindName}`);
        ctx.ui.notify(`Forked "${mindName}" → "${newName}"`, "success");
        break;
      }
      case "ingest": {
        const allMinds = store!.listMinds().filter(m => m.name !== mindName);
        if (allMinds.length === 0) {
          ctx.ui.notify("No targets to ingest into", "warning");
          return;
        }
        const targetIdx = await ctx.ui.select(
          "Ingest into:",
          allMinds.map(m => `${m.name} (${m.factCount} facts)`),
        );
        if (targetIdx === undefined) return;
        const target = allMinds[targetIdx];

        const sourceCount = store!.countActiveFacts(mindName);
        const sourceReadonly = store!.isMindReadonly(mindName);
        const retireMsg = sourceReadonly ? "" : ` and retire "${mindName}"`;
        const ok = await ctx.ui.confirm(
          "Ingest Mind",
          `Merge ${sourceCount} facts from "${mindName}" into "${target.name}" (duplicates skipped)${retireMsg}?`,
        );
        if (!ok) return;

        const result = store!.ingestMind(mindName, target.name);
        ctx.ui.notify(
          `Ingested ${result.factsIngested} facts into "${target.name}" (${result.duplicatesSkipped} duplicates skipped)`,
          "success",
        );

        if (activeMind() === mindName) {
          store!.setActiveMind(target.name === "default" ? null : target.name);
          updateStatus(ctx);
        }
        break;
      }
      case "status": {
        const statuses = ["active", "refined", "retired"] as const;
        const idx = await ctx.ui.select("New status:", [...statuses]);
        if (idx === undefined) return;
        store!.setMindStatus(mindName, statuses[idx]);
        ctx.ui.notify(`Status of "${mindName}" → ${statuses[idx]}`, "success");
        break;
      }
      case "delete": {
        const ok = await ctx.ui.confirm("Delete Mind", `Permanently delete mind "${mindName}" and all its facts?`);
        if (!ok) return;
        const wasActive = activeMind() === mindName;
        store!.deleteMind(mindName);
        if (wasActive) {
          store!.setActiveMind(null);
          updateStatus(ctx);
        }
        ctx.ui.notify(`Deleted mind: ${mindName}`, "success");
        break;
      }
    }
  }

  function updateStatus(ctx: ExtensionContext): void {
    if (!ctx.hasUI || !store) return;

    const mind = activeMind();
    const count = store.countActiveFacts(mind);
    if (mind !== "default") {
      ctx.ui.setStatus("memory", ctx.ui.theme.fg("dim", `🧠 ${mind} (${count})`));
    } else {
      ctx.ui.setStatus("memory", ctx.ui.theme.fg("dim", `🧠 ${count}`));
    }
  }

  // --- Commands ---

  pi.registerCommand("memory", {
    description: "Interactive mind manager — view, switch, create, fork, ingest memory stores",
    getArgumentCompletions: (prefix: string) => {
      const subs = ["edit", "refresh", "clear", "link", "stats"];
      const filtered = subs.filter((s) => s.startsWith(prefix));
      return filtered.length > 0 ? filtered.map((s) => ({ value: s, label: s })) : null;
    },
    handler: async (args, ctx) => {
      if (!store) {
        ctx.ui.notify("Project memory not initialized", "error");
        return;
      }

      const subcommand = args?.trim().split(/\s+/)[0] ?? "";

      switch (subcommand) {
        case "edit": {
          const mind = activeMind();
          const rendered = store.renderForInjection(mind);
          const edited = await ctx.ui.editor("Project Memory:", rendered);
          if (edited !== undefined && edited !== rendered) {
            ctx.ui.notify("Direct editing not yet supported for SQLite store. Use memory_store tool.", "warning");
          } else {
            ctx.ui.notify("No changes", "info");
          }
          return;
        }

        case "refresh": {
          startRefresh(ctx);
          return;
        }

        case "clear": {
          const mind = activeMind();
          const count = store.countActiveFacts(mind);
          const ok = await ctx.ui.confirm("Clear Memory", `Archive all ${count} active facts in "${mind}"?`);
          if (ok) {
            const facts = store.getActiveFacts(mind);
            for (const f of facts) {
              store.archiveFact(f.id);
            }
            ctx.ui.notify(`Archived ${count} facts`, "success");
            updateStatus(ctx);
          }
          return;
        }

        case "link": {
          const parts = args?.trim().split(/\s+/).slice(1) ?? [];
          if (parts.length < 1) {
            ctx.ui.notify("Usage: /memory link <path> [name]", "warning");
            return;
          }
          const linkPath = parts[0];
          const rawName = parts[1] ?? path.basename(linkPath);
          const linkName = sanitizeMindName(rawName);
          if (!linkName) {
            ctx.ui.notify("Could not derive a valid name from path", "error");
            return;
          }
          if (store.mindExists(linkName)) {
            ctx.ui.notify(`Mind "${linkName}" already exists`, "error");
            return;
          }
          // For linked minds, we'd need to import from external path
          // This is a simplified version — full link/sync needs more work
          ctx.ui.notify(`Linked mind support is being rebuilt for SQLite store`, "warning");
          return;
        }

        case "stats": {
          const mind = activeMind();
          const facts = store.getActiveFacts(mind);
          const total = facts.length;
          const bySection = new Map<string, number>();
          const bySource = new Map<string, number>();
          let totalReinforcements = 0;
          let avgConfidence = 0;

          for (const f of facts) {
            bySection.set(f.section, (bySection.get(f.section) ?? 0) + 1);
            bySource.set(f.source, (bySource.get(f.source) ?? 0) + 1);
            totalReinforcements += f.reinforcement_count;
            avgConfidence += f.confidence;
          }
          avgConfidence = total > 0 ? avgConfidence / total : 0;

          const lines = [
            `Mind: ${activeLabel()}`,
            `Active facts: ${total}`,
            `Avg confidence: ${(avgConfidence * 100).toFixed(1)}%`,
            `Avg reinforcements: ${(totalReinforcements / Math.max(total, 1)).toFixed(1)}`,
            "",
            "By section:",
            ...SECTIONS.map(s => `  ${s}: ${bySection.get(s) ?? 0}`),
            "",
            "By source:",
            ...Array.from(bySource.entries()).map(([s, n]) => `  ${s}: ${n}`),
          ];
          ctx.ui.notify(lines.join("\n"), "info");
          return;
        }
      }

      // Interactive mind manager
      const minds = store.listMinds();
      const active = activeMind();
      const items = buildMindItems(minds, active);

      const selected = await ctx.ui.custom<string | null>((tui, theme, _kb, done) => {
        const container = new Container();
        container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));
        container.addChild(new Text(
          theme.fg("accent", theme.bold(" Memory Minds ")) +
          theme.fg("dim", `(active: ${activeLabel()})`),
          1, 0,
        ));

        const selectList = new SelectList(items, Math.min(items.length + 1, 15), {
          selectedPrefix: (t) => theme.fg("accent", t),
          selectedText: (t) => theme.fg("accent", t),
          description: (t) => theme.fg("muted", t),
          scrollInfo: (t) => theme.fg("dim", t),
          noMatch: (t) => theme.fg("warning", t),
        });
        selectList.onSelect = (item) => done(item.value);
        selectList.onCancel = () => done(null);
        container.addChild(selectList);
        container.addChild(new Text(theme.fg("dim", " ↑↓ navigate • enter select/switch • esc close"), 1, 0));
        container.addChild(new DynamicBorder((s: string) => theme.fg("accent", s)));

        return {
          render: (w) => container.render(w),
          invalidate: () => container.invalidate(),
          handleInput: (data) => { selectList.handleInput(data); tui.requestRender(); },
        };
      });

      if (!selected) return;

      if (selected === "__create__") {
        const rawName = await ctx.ui.input("Mind name:");
        if (!rawName?.trim()) return;
        const name = sanitizeMindName(rawName);
        if (!name) {
          ctx.ui.notify("Name must contain at least one alphanumeric character", "error");
          return;
        }
        if (name !== rawName.trim()) {
          ctx.ui.notify(`Name sanitized to: ${name}`, "info");
        }
        if (store.mindExists(name)) {
          ctx.ui.notify(`Mind "${name}" already exists`, "error");
          return;
        }
        const desc = await ctx.ui.input("Description:");
        store.createMind(name, desc ?? "");
        const activate = await ctx.ui.confirm("Activate", `Switch to "${name}" now?`);
        if (activate) {
          store.setActiveMind(name);
          updateStatus(ctx);
          notifyMindSwitch(name, 0);
        }
        ctx.ui.notify(`Created mind: ${name}`, "success");
        return;
      }

      if (selected === "__link__") {
        ctx.ui.notify("Linked mind support is being rebuilt for SQLite store", "warning");
        return;
      }

      if (selected === "__edit__") {
        const mind = activeMind();
        const rendered = store.renderForInjection(mind);
        const edited = await ctx.ui.editor("Edit Current Mind:", rendered);
        if (edited !== undefined && edited !== rendered) {
          ctx.ui.notify("Direct editing not yet supported for SQLite store. Use memory_store tool.", "warning");
        }
        return;
      }

      if (selected === "__refresh__") {
        startRefresh(ctx);
        return;
      }

      // Selected an existing mind — show actions
      await showMindActions(ctx, selected);
    },
  });
}
