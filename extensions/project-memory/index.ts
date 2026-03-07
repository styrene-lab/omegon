/**
 * Project Memory Extension
 *
 * Persistent, cross-session project knowledge stored in SQLite with
 * confidence-decay reinforcement, semantic retrieval via local embeddings,
 * episodic session narratives, and working memory.
 *
 * Storage: .pi/memory/facts.db (SQLite with WAL mode)
 * Vectors: facts_vec / episodes_vec tables (Float32 BLOBs via Ollama embeddings)
 * Rendering: Active facts → Markdown-KV for LLM context injection
 *
 * Tools:
 *   memory_query          — Read all active memory (full dump, rendered Markdown-KV)
 *   memory_recall         — Semantic search over active facts (targeted retrieval)
 *   memory_store          — Add a fact (with conflict detection)
 *   memory_supersede      — Replace a fact atomically
 *   memory_archive        — Archive stale/redundant facts by ID
 *   memory_search_archive — FTS keyword search over archived facts
 *   memory_connect        — Create relationships between facts
 *   memory_compact        — Trigger context compaction + memory reload
 *   memory_episodes       — Search session narratives (episodic memory)
 *   memory_focus          — Pin facts to working memory
 *   memory_release        — Clear working memory
 *
 * Cognitive features:
 *   - Semantic retrieval via local embeddings (Ollama qwen3-embedding)
 *   - Contextual auto-injection (relevant facts only, not full dump)
 *   - Working memory buffer (pinned facts survive compaction)
 *   - Conflict detection at store time (flags similar but not identical facts)
 *   - Episodic memory (session narratives generated at shutdown)
 *   - Background vector indexing (embeds facts async on session start)
 *
 * Commands:
 *   /memory               — Interactive mind manager
 *   /memory edit           — Edit current mind in editor
 *   /memory refresh        — Re-evaluate and prune memory
 *   /memory clear          — Reset current mind
 *   /memory stats          — Show memory statistics
 *
 * Background extraction via subagent outputs JSONL actions.
 */

import * as path from "node:path";
import * as os from "node:os";
import type { ExtensionAPI, ExtensionContext, ExtensionCommandContext, SessionMessageEntry } from "@mariozechner/pi-coding-agent";
import { DynamicBorder } from "@mariozechner/pi-coding-agent";
import { StringEnum } from "../lib/typebox-helpers";
import { Type } from "@sinclair/typebox";
import { Container, type SelectItem, SelectList, Text } from "@mariozechner/pi-tui";
import { FactStore, parseExtractionOutput, GLOBAL_DECAY, type MindRecord, type Fact } from "./factstore.ts";
import { embed, isEmbeddingAvailable, MODEL_DIMS } from "./embeddings.ts";
import { DEFAULT_CONFIG, type MemoryConfig } from "./types.ts";
import {
  type ExtractionTriggerState,
  createTriggerState,
  shouldExtract,
} from "./triggers.ts";
import { runExtractionV2, runGlobalExtraction, killActiveExtraction, killAllSubprocesses, generateEpisode, generateEpisodeDirect } from "./extraction-v2.ts";
import { migrateToFactStore, needsMigration, markMigrated } from "./migration.ts";
import { SECTIONS } from "./template.ts";
import { serializeConversation, convertToLlm } from "@mariozechner/pi-coding-agent";
import { sharedState } from "../shared-state.ts";

/** Map abstract effort model tiers to concrete cloud model IDs for extraction. */
const EFFORT_EXTRACTION_MODELS: Record<string, string> = {
  opus: "claude-opus-4-6",
  sonnet: "claude-sonnet-4-6",
};

// ---------------------------------------------------------------------------
// Compaction prompt constants (mirrors pi's internal prompts for local-model fallback)
// ---------------------------------------------------------------------------

const COMPACTION_SYSTEM_PROMPT = "You are a context summarization assistant. Your task is to read a conversation between a user and an AI coding assistant, then produce a structured summary following the exact format specified.\n\nDo NOT continue the conversation. Do NOT respond to any questions in the conversation. ONLY output the structured summary.";

const COMPACTION_INITIAL_PROMPT = `Create a structured context checkpoint summary that another LLM will use to continue the work.

Use this EXACT format:

## Goal
[What is the user trying to accomplish?]

## Constraints & Preferences
- [Any constraints, preferences, or requirements mentioned]

## Progress
### Done
- [x] [Completed tasks/changes]

### In Progress
- [ ] [Current work]

### Blocked
- [Issues preventing progress, if any]

## Key Decisions
- **[Decision]**: [Brief rationale]

## Next Steps
1. [Ordered list of what should happen next]

## Critical Context
- [Any data, examples, or references needed to continue]

Keep each section concise. Preserve exact file paths, function names, and error messages.`;

const COMPACTION_UPDATE_PROMPT = `Update the existing structured summary with new information from the conversation. RULES:
- PRESERVE all existing information from the previous summary
- ADD new progress, decisions, and context
- UPDATE Progress: move items from "In Progress" to "Done" when completed
- UPDATE "Next Steps" based on what was accomplished

Use the same format (Goal, Constraints & Preferences, Progress, Key Decisions, Next Steps, Critical Context).
Keep each section concise. Preserve exact file paths, function names, and error messages.`;

const COMPACTION_TURN_PREFIX_PROMPT = `This is the PREFIX of a turn that was too large to keep. The SUFFIX (recent work) is retained.

Summarize the prefix to provide context for the retained suffix:

## Original Request
[What did the user ask for?]

## Early Progress
- [Key decisions and work done in the prefix]

## Context for Suffix
- [Information needed to understand the retained recent work]

Be concise. Focus on what's needed to understand the kept suffix.`;

// ---------------------------------------------------------------------------
// Ollama helpers for local-model compaction fallback
// ---------------------------------------------------------------------------

const OLLAMA_URL = () => process.env.OLLAMA_HOST || process.env.LOCAL_INFERENCE_URL || "http://localhost:11434";

/** Embedding model names that must not be used for chat completions */
const EMBEDDING_MODEL_PATTERN = /embed|embedding/i;

/** Preferred models for summarization, in priority order */
const PREFERRED_CHAT_MODELS = [
  "qwen3:32b", "devstral-small-2:24b", "qwen3:30b", "nemotron-3-nano:30b",
  "qwen3", "devstral-small", "nemotron",
];

/**
 * Discover a chat-capable local model via Ollama's OpenAI-compatible API.
 * Returns model ID or null if unavailable.
 */
async function discoverLocalChatModel(): Promise<string | null> {
  try {
    const resp = await fetch(`${OLLAMA_URL()}/v1/models`, { signal: AbortSignal.timeout(2_000) });
    if (!resp.ok) return null;
    const data = await resp.json() as { data?: Array<{ id: string }> };
    const available = (data.data?.map((m: { id: string }) => m.id) ?? [])
      .filter((id: string) => !EMBEDDING_MODEL_PATTERN.test(id));
    if (available.length === 0) return null;

    // Try preferred models first (startsWith for exact matching)
    for (const pref of PREFERRED_CHAT_MODELS) {
      const found = available.find((id: string) => id.startsWith(pref));
      if (found) return found;
    }
    return available[0]; // Any non-embedding model
  } catch {
    return null;
  }
}

/**
 * Send a chat completion to Ollama. Returns trimmed content or null.
 */
async function ollamaChat(
  model: string,
  systemPrompt: string,
  userPrompt: string,
  opts: { maxTokens?: number; signal?: AbortSignal },
): Promise<string | null> {
  const resp = await fetch(`${OLLAMA_URL()}/v1/chat/completions`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      model,
      messages: [
        { role: "system", content: systemPrompt },
        { role: "user", content: userPrompt },
      ],
      max_tokens: opts.maxTokens ?? 4096,
      temperature: 0.3,
      // Request a reasonable context window for the local model
      num_ctx: 32768,
    }),
    signal: opts.signal,
  });
  if (!resp.ok) return null;
  const data = await resp.json() as { choices?: Array<{ message?: { content?: string } }> };
  const content = data.choices?.[0]?.message?.content?.trim();
  return content || null;
}

/**
 * Format file operations for appending to compaction summary.
 * Mirrors pi core's formatFileOperations but inlined since it's not exported.
 */
function formatFileOps(fileOps: { read: Set<string>; edited: Set<string>; written: Set<string> }): string {
  const modified = new Set([...fileOps.edited, ...fileOps.written]);
  const readOnly = [...fileOps.read].filter(f => !modified.has(f)).sort();
  const modifiedFiles = [...modified].sort();

  const sections: string[] = [];
  if (readOnly.length > 0) sections.push(`<read-files>\n${readOnly.join("\n")}\n</read-files>`);
  if (modifiedFiles.length > 0) sections.push(`<modified-files>\n${modifiedFiles.join("\n")}\n</modified-files>`);
  return sections.length > 0 ? `\n\n${sections.join("\n\n")}` : "";
}

/**
 * Build details object for CompactionResult from file operations.
 */
function buildFileDetails(fileOps: { read: Set<string>; edited: Set<string>; written: Set<string> }) {
  const modified = new Set([...fileOps.edited, ...fileOps.written]);
  const readFiles = [...fileOps.read].filter(f => !modified.has(f)).sort();
  const modifiedFiles = [...modified].sort();
  return { readFiles, modifiedFiles };
}

/**
 * Compute degeneracy pressure as an exponential curve from onset to warning threshold.
 * Returns 0 below onset, 1 at warning threshold, exponential growth between.
 *
 * The curve is: pressure = (e^(k*t) - 1) / (e^k - 1)
 * where t = (pct - onset) / (warning - onset), normalized 0→1
 * and k controls steepness (higher = more exponential, 3 gives ~20:1 ratio)
 */
function computeDegeneracyPressure(
  pct: number,
  onset: number,
  warning: number,
  k = 3,
): number {
  if (pct < onset) return 0;
  if (pct >= warning) return 1;
  const t = (pct - onset) / (warning - onset);
  return (Math.exp(k * t) - 1) / (Math.exp(k) - 1);
}

/**
 * Map degeneracy pressure (0→1) to a context-appropriate guidance message.
 * Messages escalate in urgency and specificity as pressure increases.
 */
function pressureGuidance(pressure: number, pct: number): string | null {
  if (pressure <= 0) return null;

  // Five levels of escalating guidance
  if (pressure < 0.15) {
    return `📊 Context: ${pct}% — Wrap up current threads before starting new large tasks.`;
  }
  if (pressure < 0.35) {
    return `📊 Context: ${pct}% — Finish current work, then compact before starting anything new.`;
  }
  if (pressure < 0.6) {
    return `📊 Context: ${pct}% (elevated) — Complete your current task and call **memory_compact**. Avoid starting new multi-step work.`;
  }
  if (pressure < 0.85) {
    return `⚠️ Context: ${pct}% (high) — You should **memory_compact** now unless you're mid-implementation with uncommitted changes. New tasks will not fit.`;
  }
  return `🔴 Context: ${pct}% (critical) — Call **memory_compact** immediately. All stored facts and working memory survive compaction.`;
}

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
  /** Set by /exit handler when episode generation is done pre-goodbye */
  let exitEpisodeDone = false;
  let consecutiveExtractionFailures = 0;
  let memoryDir = "";
  const globalMemoryDir = path.join(os.homedir(), ".pi", "memory");

  // --- Context Pressure State ---
  let compactionWarned = false;   // true after we've injected a warning this cycle
  let autoCompacted = false;      // true after auto-compaction triggered this cycle
  let compactionRetryCount = 0;   // consecutive compaction failures this session
  let useLocalCompaction = false; // set true after cloud failure to trigger local fallback

  // --- Embedding / Semantic Retrieval State ---
  let embeddingAvailable = false;
  let embeddingModel: string | undefined;

  // --- Working Memory Buffer (session-scoped) ---
  /** Fact IDs the agent has explicitly recalled or stored this session */
  const workingMemory = new Set<string>();
  const WORKING_MEMORY_CAP = 25;

  /** Get the active mind name (null = default) */
  /**
   * Apply the current effort tier's extraction override to a MemoryConfig.
   * Called at extraction call-time so mid-session /effort switches take effect
   * immediately without requiring a session restart.
   * Returns a new config object (does not mutate).
   */
  function applyEffortToCfg(cfg: MemoryConfig): MemoryConfig {
    const effort = sharedState.effort;
    if (!effort) return cfg;
    if (effort.extraction === "local") return cfg;
    const model = EFFORT_EXTRACTION_MODELS[effort.extraction];
    if (!model) return cfg;
    return { ...cfg, extractionModel: model };
  }

  function activeMind(): string {
    return store?.getActiveMind() ?? "default";
  }

  function activeLabel(): string {
    const mind = store?.getActiveMind();
    return mind ?? "default";
  }

  // --- Embedding Helpers ---

  /** Embed a single text, returning the vector or null if unavailable */
  async function embedText(text: string): Promise<Float32Array | null> {
    if (!embeddingAvailable) return null;
    const result = await embed(text, { model: embeddingModel });
    return result?.embedding ?? null;
  }

  /**
   * Embed a fact and store its vector. Returns true if successful.
   * No-op if embeddings are unavailable or fact already has a vector.
   */
  async function embedFact(factId: string): Promise<boolean> {
    if (!embeddingAvailable || !store) return false;
    if (store.hasFactVector(factId)) return true;
    const fact = store.getFact(factId);
    if (!fact || fact.status !== "active") return false;
    const result = await embed(
      `[${fact.section}] ${fact.content}`,
      { model: embeddingModel },
    );
    if (!result) return false;
    store.storeFactVector(factId, result.embedding, result.model);
    return true;
  }

  /**
   * Background index: embed all active facts missing vectors.
   * Runs async, doesn't block session. Reports progress via status bar.
   */
  async function backgroundIndexFacts(ctx: ExtensionContext): Promise<void> {
    if (!embeddingAvailable || !store) return;
    const mind = activeMind();
    const missing = store.getFactsMissingVectors(mind);
    if (missing.length === 0) return;

    let indexed = 0;
    for (const factId of missing) {
      if (!sessionActive) break; // Stop if session is shutting down
      const ok = await embedFact(factId);
      if (ok) indexed++;
    }

    // Also index global store facts
    if (globalStore) {
      const globalMind = globalStore.getActiveMind() ?? "default";
      const globalMissing = globalStore.getFactsMissingVectors(globalMind);
      for (const factId of globalMissing) {
        if (!sessionActive) break;
        const fact = globalStore.getFact(factId);
        if (!fact || fact.status !== "active") continue;
        const result = await embed(
          `[${fact.section}] ${fact.content}`,
          { model: embeddingModel },
        );
        if (result) {
          globalStore.storeFactVector(factId, result.embedding, result.model);
        }
      }
    }
  }

  /** Add fact IDs to working memory, evicting oldest if over cap */
  function addToWorkingMemory(...ids: string[]): void {
    for (const id of ids) {
      // If already present, remove and re-add to refresh position
      workingMemory.delete(id);
      workingMemory.add(id);
    }
    // Evict oldest if over cap
    while (workingMemory.size > WORKING_MEMORY_CAP) {
      const oldest = workingMemory.values().next().value;
      if (oldest) workingMemory.delete(oldest);
    }
  }

  // --- Lifecycle ---

  pi.on("session_start", async (_event, ctx) => {
    memoryDir = path.join(ctx.cwd, ".pi", "memory");

    // Initialize project store
    try {
      if (needsMigration(memoryDir)) {
        store = new FactStore(memoryDir);
        const result = migrateToFactStore(memoryDir, store);
        markMigrated(memoryDir);
        if (ctx.hasUI) {
          const msg = `Memory migrated to SQLite: ${result.factsImported} facts imported, ${result.archiveFactsImported} archive facts, ${result.mindsImported} minds`;
          ctx.ui.notify(msg, "info");
        }
      } else {
        store = new FactStore(memoryDir);
      }
    } catch (err: any) {
      const hint = /DLOPEN|NODE_MODULE_VERSION|compiled against/.test(err.message)
        ? "\nFix: run `npm rebuild better-sqlite3` in the pi-kit directory, then restart."
        : "";
      ctx.ui.notify(
        `[project-memory] Failed to open project database: ${err.message}${hint}`,
        "error"
      );
      // store stays null — tools will report "not initialized"
    }

    // Initialize global store (user-level, shared across projects)
    // Uses global.db to avoid collision with project facts.db when CWD is ~/
    try {
      globalStore = new FactStore(globalMemoryDir, { decay: GLOBAL_DECAY, dbName: "global.db" });
    } catch (err: any) {
      const hint = /DLOPEN|NODE_MODULE_VERSION|compiled against/.test(err.message)
        ? "\nFix: run `npm rebuild better-sqlite3` in the pi-kit directory, then restart."
        : "";
      ctx.ui.notify(
        `[project-memory] Failed to open global database: ${err.message}${hint}`,
        "error"
      );
      // globalStore stays null — global features degrade gracefully
    }

    // Auto-import: always merge facts.jsonl into DB on startup.
    // importFromJsonl deduplicates by content_hash — existing facts get reinforced,
    // new facts get inserted. This is safe to run every session because it's additive.
    //
    // Previous mtime-based gating was broken: new FactStore() creates/opens the DB
    // (setting mtime=NOW) before this check runs, so jsonlMtime > dbMtime was always
    // false for fresh DBs, silently skipping import and then overwriting the JSONL
    // on shutdown with only the current session's facts.
    const jsonlPath = path.join(memoryDir, "facts.jsonl");
    try {
      const fsSync = await import("node:fs");
      if (fsSync.existsSync(jsonlPath)) {
        const jsonl = fsSync.readFileSync(jsonlPath, "utf8");
        if (jsonl.trim()) {
          const result = store!.importFromJsonl(jsonl);
          if (ctx.hasUI && (result.factsAdded > 0 || result.edgesAdded > 0)) {
            ctx.ui.notify(
              `Memory sync: +${result.factsAdded} facts, ${result.factsReinforced} reinforced, +${result.edgesAdded} edges`,
              "info"
            );
          }
        }
      }
    } catch {
      // Best effort — don't block startup
    }

    // Ensure .gitignore covers memory/ db files but allows facts.jsonl
    const gitignorePath = path.join(memoryDir, "..", ".gitignore");
    try {
      const fs = await import("node:fs");
      let existing = fs.existsSync(gitignorePath)
        ? fs.readFileSync(gitignorePath, "utf8")
        : "";
      let changed = false;
      if (!existing.includes("memory/*.db")) {
        existing += (existing.endsWith("\n") || existing === "" ? "" : "\n") + "memory/*.db\nmemory/*.db-wal\nmemory/*.db-shm\n";
        changed = true;
      }
      // Remove old blanket "memory/" ignore if present (we now want facts.jsonl tracked)
      if (existing.includes("memory/\n")) {
        existing = existing.replace("memory/\n", "");
        changed = true;
      }
      if (changed) {
        fs.writeFileSync(gitignorePath, existing, "utf8");
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
    compactionWarned = false;
    autoCompacted = false;
    workingMemory.clear();

    // Apply effort-tier overrides to extraction and compaction config.
    // sharedState.effort is written by the effort extension's session_start,
    // which fires before ours (effort is registered earlier in package.json).
    config = { ...DEFAULT_CONFIG };
    const effort = sharedState.effort;
    if (effort) {
      // Extraction: tiers 1-5 use local (devstral default), tiers 6-7 use cloud
      if (effort.extraction !== "local") {
        const model = EFFORT_EXTRACTION_MODELS[effort.extraction];
        if (model) config.extractionModel = model;
      }
      // Compaction: tiers 1-5 stay local-first, tiers 6-7 defer to cloud
      if (effort.compaction !== "local") {
        config.compactionLocalFirst = false;
      }
    }

    // Detect embedding availability and start background indexing
    try {
      const embedStatus = await isEmbeddingAvailable();
      embeddingAvailable = embedStatus.available;
      embeddingModel = embedStatus.model;
      if (embeddingAvailable && embeddingModel) {
        // Purge vectors from a different model (dimension mismatch)
        const expectedDims = MODEL_DIMS[embeddingModel];
        if (expectedDims && store) {
          const purged = store.purgeStaleVectors(expectedDims);
          if (purged > 0 && ctx.hasUI) {
            ctx.ui.notify(`Purged ${purged} stale vectors (model changed to ${embeddingModel})`, "info");
          }
        }
        if (expectedDims && globalStore) {
          globalStore.purgeStaleVectors(expectedDims);
        }
        // Fire-and-forget background indexing — don't block session start
        backgroundIndexFacts(ctx).catch(() => {});
      }
    } catch {
      embeddingAvailable = false;
    }

    updateStatus(ctx);
  });

  pi.on("session_shutdown", async (_event, ctx) => {
    sessionActive = false;

    // Kill any running extraction subprocess immediately.
    // On /reload, the old module is discarded — orphaned subprocesses with dangling
    // pipe listeners corrupt terminal state (ANSI escape sequences leak to stdout).
    // killAllSubprocesses covers both extraction and episode generation processes.
    killAllSubprocesses();

    // Wait for the extraction promise to fully settle after kill.
    // Must not close DB until the promise resolves/rejects — otherwise the
    // close event handler or processExtraction() hits a closed DB.
    if (activeExtractionPromise) {
      if (ctx.hasUI) {
        ctx.ui.setStatus("memory", ctx.ui.theme.fg("dim", "saving memory…"));
      }
      try { await activeExtractionPromise; } catch { /* expected after kill */ }
    }

    // Episode generation: skip if /exit already did it (fast path).
    // For non-/exit shutdowns (ctrl-c, /reload), do a quick direct attempt only.
    if (!exitEpisodeDone && store) {
      try {
        const mind = activeMind();
        const branch = ctx.sessionManager.getBranch();
        const messages = branch
          .filter((e): e is SessionMessageEntry => e.type === "message")
          .map((e) => e.message);

        if (messages.length > 5) {
          const recentMessages = messages.slice(-20);
          const serialized = serializeConversation(convertToLlm(recentMessages));
          const sessionFactIds = [...workingMemory];

          // Direct Ollama only — no subprocess spawn during shutdown
          const episodeOutput = await generateEpisodeDirect(serialized, config);
          if (episodeOutput) {
            const today = new Date().toISOString().split("T")[0];
            const episodeId = store.storeEpisode({
              mind,
              title: episodeOutput.title,
              narrative: episodeOutput.narrative,
              date: today,
              factIds: sessionFactIds.filter(id => store!.getFact(id)?.status === "active"),
            });

            if (embeddingAvailable) {
              const vec = await embedText(`${episodeOutput.title} ${episodeOutput.narrative}`);
              if (vec) store.storeEpisodeVector(episodeId, vec, embeddingModel!);
            }
          }
        }
      } catch {
        // Best effort — don't block shutdown
      }
    }

    // JSONL export + DB close (fast — synchronous I/O, ~50ms)
    if (store) {
      try {
        const fsSync = await import("node:fs");
        const jsonlPath = path.join(memoryDir, "facts.jsonl");
        const jsonl = store.exportToJsonl();
        fsSync.writeFileSync(jsonlPath, jsonl, "utf8");
      } catch {
        // Best effort — don't block shutdown
      }
    }

    store?.close(); globalStore?.close();
  });

  // --- Local-model compaction ---
  // Two modes:
  // 1. compactionLocalFirst=true (default): intercept ALL compactions, try local first.
  //    Cloud is only used if local model is unavailable (Ollama not running).
  // 2. compactionLocalFirst=false: only intercept when useLocalCompaction flag is set
  //    (after cloud failure). Cloud gets first attempt; local is the safety net.
  pi.on("session_before_compact", async (event, ctx) => {
    // Re-read effort state at intercept time so mid-session /effort switches take effect.
    const liveCompactionLocal = sharedState.effort
      ? sharedState.effort.compaction === "local"
      : config.compactionLocalFirst;
    const shouldIntercept = liveCompactionLocal || useLocalCompaction;
    if (!shouldIntercept || !config.compactionLocalFallback) return;
    useLocalCompaction = false; // consume the flag if it was set

    const prep = event.preparation;
    if (!prep || prep.messagesToSummarize.length === 0) return;

    const localModel = await discoverLocalChatModel();
    if (!localModel) return; // No local model — cloud retry will also fail, but that's logged

    if (ctx.hasUI) {
      ctx.ui.notify(
        config.compactionLocalFirst
          ? "Compacting via local model"
          : "Cloud compaction failed — falling back to local model",
        config.compactionLocalFirst ? "info" : "warning",
      );
    }

    const timeoutSignal = AbortSignal.timeout(config.compactionLocalTimeout);
    const combinedSignal = AbortSignal.any([event.signal, timeoutSignal]);

    // Build summarization prompt
    const llmMessages = convertToLlm(prep.messagesToSummarize);
    let conversationText = serializeConversation(llmMessages);

    // Truncate to ~60k chars (~15k tokens) to fit local model context windows.
    // Most local models have 8k-32k context; we leave room for system prompt + output.
    const MAX_CONVERSATION_CHARS = 60_000;
    if (conversationText.length > MAX_CONVERSATION_CHARS) {
      conversationText = "...[earlier conversation truncated]...\n\n"
        + conversationText.slice(-MAX_CONVERSATION_CHARS);
    }

    let promptText = `<conversation>\n${conversationText}\n</conversation>\n\n`;
    if (prep.previousSummary) {
      promptText += `<previous-summary>\n${prep.previousSummary}\n</previous-summary>\n\n`;
    }

    // Inject project memory context for richer summaries
    if (store) {
      const mind = activeMind();
      const facts = store.getActiveFacts(mind);
      if (facts.length > 0) {
        const factLines = facts.slice(0, 30).map((f: Fact) => `- [${f.section}] ${f.content}`).join("\n");
        promptText += `<project-memory>\n${factLines}\n</project-memory>\n\n`;
        promptText += "The project memory above provides persistent context. Reference relevant facts in your summary.\n\n";
      }
    }

    const basePrompt = prep.previousSummary
      ? COMPACTION_UPDATE_PROMPT
      : COMPACTION_INITIAL_PROMPT;

    const customInstructions = event.customInstructions;
    promptText += customInstructions ? `${basePrompt}\n\nAdditional focus: ${customInstructions}` : basePrompt;

    // Handle split turn prefix
    let turnPrefixSummary = "";
    if (prep.isSplitTurn && prep.turnPrefixMessages.length > 0) {
      const prefixMessages = convertToLlm(prep.turnPrefixMessages);
      let prefixText = serializeConversation(prefixMessages);
      if (prefixText.length > MAX_CONVERSATION_CHARS) {
        prefixText = "...[truncated]...\n\n" + prefixText.slice(-MAX_CONVERSATION_CHARS);
      }
      const prefixPrompt = `<conversation>\n${prefixText}\n</conversation>\n\n${COMPACTION_TURN_PREFIX_PROMPT}`;

      try {
        const prefixResp = await ollamaChat(localModel, COMPACTION_SYSTEM_PROMPT, prefixPrompt, {
          maxTokens: 2048, signal: combinedSignal,
        });
        if (prefixResp) turnPrefixSummary = prefixResp;
      } catch {
        // If turn prefix fails, continue without it
      }
    }

    // Generate main summary via local model
    try {
      const summary = await ollamaChat(localModel, COMPACTION_SYSTEM_PROMPT, promptText, {
        maxTokens: 4096, signal: combinedSignal,
      });

      if (!summary) return; // Empty response — compaction fails entirely (already retried)

      let fullSummary = summary;
      if (turnPrefixSummary) {
        fullSummary += `\n\n---\n\n**Turn Context (split turn):**\n\n${turnPrefixSummary}`;
      }

      // Append file operations (mirrors pi core behavior)
      fullSummary += formatFileOps(prep.fileOps);

      return {
        compaction: {
          summary: fullSummary,
          firstKeptEntryId: prep.firstKeptEntryId,
          tokensBefore: prep.tokensBefore,
          details: buildFileDetails(prep.fileOps),
        },
      };
    } catch (err) {
      // Local model failed — compaction fails entirely
      console.error(`[project-memory] Local model compaction failed: ${err instanceof Error ? err.message : String(err)}`);
      return;
    }
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
    compactionWarned = false;
    autoCompacted = false;
    compactionRetryCount = 0; // successful compaction resets retry counter

    // Resume the agent after ANY compaction (pi-initiated or extension-initiated).
    // Pi's built-in auto-compaction at threshold doesn't resume the agent — it just
    // compacts and goes idle. Without this, the agent hangs after compaction.
    //
    // IMPORTANT: Use setTimeout to avoid reentrancy. In pi's auto-compaction path,
    // session_compact fires from within _handleAgentEvent(agent_end). Calling
    // agent.prompt() synchronously here would cause reentrant event processing
    // (agent_start inside agent_end handling). The 100ms delay matches pi's own
    // pattern for post-compaction resume (see _runAutoCompaction's setTimeout).
    setTimeout(() => {
      pi.sendMessage(
        {
          customType: "compaction-resume",
          content: [
            "Context was compacted to free space. Your project memory and working memory are intact.",
            "",
            "**Resume your previous task.** The compaction summary above preserves your progress.",
            "If you need to recall specific facts, use `memory_recall(query)` for targeted retrieval.",
          ].join("\n"),
          display: false,
        },
        { triggerTurn: true },
      );
    }, 100);
  });

  // --- Compaction retry with local model fallback ---
  // Pi's own auto-compaction handles triggering at its threshold (~contextWindow - reserveTokens).
  // When cloud compaction fails (overloaded_error), pi doesn't retry. On the next
  // tool_execution_end, if context is still above our threshold, we trigger compaction
  // ourselves with the local model fallback enabled.
  //
  // Flow: pi auto-compact (cloud) → fails → next tool_execution_end → still over threshold
  //       → we set useLocalCompaction=true → ctx.compact() → session_before_compact
  //       → local model generates summary → success → resume relay

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

    // Re-apply effort override at call-time so mid-session /effort switches take effect
    // without requiring a session restart.
    const activeCfg = applyEffortToCfg(cfg);

    const serialized = serializeConversation(convertToLlm(recentMessages));
    const rawOutput = await runExtractionV2(ctx.cwd, currentFacts, serialized, activeCfg);

    if (!rawOutput.trim()) return;

    const actions = parseExtractionOutput(rawOutput);
    if (actions.length > 0) {
      const result = store.processExtraction(mind, actions);

      // Embed newly created facts (fire-and-forget)
      if (result.newFactIds.length > 0 && embeddingAvailable) {
        for (const id of result.newFactIds) {
          embedFact(id).catch(() => {});
        }
      }

      // Phase 2: Global extraction — if new facts were created and global store exists
      if (result.newFactIds.length > 0 && globalStore && cfg.globalExtractionEnabled) {
        try {
          const newFacts = result.newFactIds
            .map(id => store!.getFact(id))
            .filter((f): f is NonNullable<typeof f> => f !== null);

          if (newFacts.length > 0) {
            const globalMind = globalStore.getActiveMind() ?? "default";
            const globalFacts = globalStore.getActiveFacts(globalMind);
            const globalEdges = globalStore.getActiveEdges();

            const globalRawOutput = await runGlobalExtraction(
              ctx.cwd, newFacts, globalFacts, globalEdges, activeCfg,
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
          const msg = (err as Error).message ?? "";
          const isRateLimit = /\b429\b/.test(msg) || msg.includes("rate_limit_error");
          if (isRateLimit) {
            // Rate limited — silently skip, will retry next cycle
          } else if (ctx.hasUI) {
            const short = msg.length > 120 ? msg.slice(0, 120) + "…" : msg;
            ctx.ui.notify(`Global extraction failed: ${short}`, "warning");
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
          applyEffortToCfg(config),
        );
        if (rawOutput.trim()) {
          const actions = parseExtractionOutput(rawOutput);
          if (actions.length > 0) {
            const result = store!.processExtraction(mind, actions);
            ctx.ui.notify(
              `Memory refreshed: ${result.added} added, ${result.reinforced} reinforced`,
              "info",
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

    // --- Contextual Auto-Injection ---
    // If embeddings are available and we have a user message, inject only
    // relevant facts + core sections. Otherwise fall back to full dump.
    let rendered: string;
    let injectionMode = "full";

    const userText = (event as any).prompt ?? "";

    const vectorCount = store.countFactVectors(mind);
    const canDoSemantic = embeddingAvailable && vectorCount >= factCount * 0.5 && userText.length > 10;

    if (canDoSemantic && factCount > 20) {
      // Semantic injection: core sections always + top-k relevant by query
      const queryVec = await embedText(userText);
      if (queryVec) {
        injectionMode = "semantic";
        const CORE_SECTIONS = ["Constraints", "Specs"];
        const allFacts = store.getActiveFacts(mind);
        const coreFacts = allFacts.filter(f => CORE_SECTIONS.includes(f.section));
        const coreIds = new Set(coreFacts.map(f => f.id));

        // Semantic search for most relevant non-core facts
        const semanticHits = store.semanticSearch(queryVec, mind, { k: 20, minSimilarity: 0.3 });
        const relevantFacts = semanticHits.filter(f => !coreIds.has(f.id));

        // Working memory facts get priority
        const wmFacts = [...workingMemory]
          .map(id => store!.getFact(id))
          .filter((f): f is Fact => f !== null && f.status === "active" && !coreIds.has(f.id));

        // Merge: core + working memory + semantic hits (deduped, capped)
        const injectedIds = new Set<string>();
        const injectedFacts: Fact[] = [];

        for (const f of coreFacts) {
          injectedFacts.push(f);
          injectedIds.add(f.id);
        }
        for (const f of wmFacts) {
          if (!injectedIds.has(f.id)) {
            injectedFacts.push(f);
            injectedIds.add(f.id);
          }
        }
        for (const f of relevantFacts) {
          if (!injectedIds.has(f.id) && injectedFacts.length < 30) {
            injectedFacts.push(f);
            injectedIds.add(f.id);
          }
        }

        rendered = store.renderFactList(injectedFacts, { showIds: false });
      } else {
        rendered = store.renderForInjection(mind);
      }
    } else {
      rendered = store.renderForInjection(mind);
    }

    // Include global knowledge if available
    let globalSection = "";
    if (globalStore) {
      const globalMind = globalStore.getActiveMind() ?? "default";
      const globalFactCount = globalStore.countActiveFacts(globalMind);
      if (globalFactCount > 0) {
        const globalRendered = globalStore.renderForInjection(globalMind, { maxFacts: 15, maxEdges: 0 });
        globalSection = `\n\n<!-- Global Knowledge — cross-project facts and connections -->\n${globalRendered}`;
      }
    }

    // Include recent episodes if available
    let episodeSection = "";
    const episodeCount = store.countEpisodes(mind);
    if (episodeCount > 0) {
      const recentEpisodes = store.getEpisodes(mind, 3);
      if (recentEpisodes.length > 0) {
        const episodeLines = recentEpisodes.map(e =>
          `### ${e.date}: ${e.title}\n${e.narrative}`
        );
        episodeSection = `\n\n## Recent Sessions\n_Episodic memory — what happened and why_\n\n${episodeLines.join("\n\n")}`;
      }
    }

    const memoryTools = embeddingAvailable
      ? "Use **memory_recall(query)** to semantically search for specific knowledge. " +
        "Use **memory_store** to persist important discoveries. " +
        "Use **memory_episodes(query)** to search session narratives."
      : "Use **memory_query** to read accumulated knowledge about this project. " +
        "Use **memory_store** to persist important discoveries (architecture decisions, constraints, patterns, known issues). " +
        "Use **memory_search_archive** to search older archived facts.";

    const injectionNote = injectionMode === "semantic"
      ? ` Showing ${injectionMode} subset — use memory_recall for more.`
      : "";

    // Context pressure — continuous gradient from onset through warning
    let pressureWarning = "";
    if (!autoCompacted) {
      const usage = ctx.getContextUsage();
      const pct = usage?.percent != null ? Math.round(usage.percent) : null;
      if (pct !== null) {
        const pressure = computeDegeneracyPressure(
          pct,
          config.pressureOnsetPercent,
          config.compactionWarningPercent,
        );
        const guidance = pressureGuidance(pressure, pct);
        if (guidance) {
          pressureWarning = `\n\n${guidance}` +
            (compactionWarned
              ? ` Auto-compaction triggers at ${config.compactionAutoPercent}%.`
              : "");
        }
      }
    }

    const injectionContent = [
      `Project memory available${mindLabel} (${factCount} facts from this and previous sessions).${injectionNote}`,
      memoryTools + "\n\n",
      rendered,
      episodeSection,
      globalSection,
      pressureWarning,
    ].join(" ");

    // Estimate token count (~4 chars per token) and publish for status-bar
    sharedState.memoryTokenEstimate = Math.round(injectionContent.length / 4);

    return {
      message: {
        customType: "project-memory",
        content: injectionContent,
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

    // --- Context Pressure: Auto-compact ---
    // Pi's built-in auto-compaction triggers at ~92% (contextWindow - reserveTokens).
    // We trigger earlier at compactionAutoPercent (default 85%) as a safety net.
    //
    // With compactionLocalFirst=true (default): session_before_compact intercepts
    // all attempts and tries local first. Cloud is fallback if Ollama unavailable.
    // With compactionLocalFirst=false: first attempt uses cloud. On failure,
    // useLocalCompaction flag is set for the retry to route through local.
    const pct = usage.percent ?? 0;
    if (pct >= config.compactionAutoPercent && !autoCompacted && compactionRetryCount < config.compactionRetryLimit) {
      autoCompacted = true;
      const isRetry = compactionRetryCount > 0;
      if (isRetry) {
        useLocalCompaction = true; // Previous cloud attempt failed — use local model
        console.error(`[project-memory] Retrying compaction with local model (attempt ${compactionRetryCount + 1})`);
      }
      if (ctx.hasUI) {
        ctx.ui.notify(
          isRetry
            ? `Retrying compaction via local model (attempt ${compactionRetryCount + 1})…`
            : `Context at ${Math.round(pct)}% — auto-compacting to preserve session continuity.`,
          "warning",
        );
      }
      ctx.compact({
        customInstructions: "Session hit auto-compaction threshold. Preserve recent work context and any in-progress task state.",
        // Resume is handled centrally in session_compact handler (covers all compaction sources).
        onError: (err: Error) => {
          compactionRetryCount++;
          autoCompacted = false; // allow retry on next tool_execution_end
          console.error(`[project-memory] Compaction failed (attempt ${compactionRetryCount}/${config.compactionRetryLimit}): ${err.message}`);
          if (compactionRetryCount >= config.compactionRetryLimit && ctx.hasUI) {
            ctx.ui.notify("Compaction failed after max retries. Context may be degraded.", "error");
          }
        },
      });
    } else if (pct >= config.compactionWarningPercent && !compactionWarned) {
      // Mark warning — will be injected via before_agent_start
      compactionWarned = true;
    }

    if (shouldExtract(triggerState, usage.tokens ?? 0, config, consecutiveExtractionFailures)) {
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
    promptSnippet: "Read accumulated project knowledge (architecture, decisions, constraints, patterns)",
    promptGuidelines: [
      "Use memory_recall(query) for targeted semantic retrieval instead of loading all facts",
      "Use memory_query only when you need the complete picture — memory_recall is more efficient",
      "Use memory_store to persist important discoveries — facts survive across sessions",
      "Use memory_episodes(query) to retrieve session narratives for context about past work",
    ],
    parameters: Type.Object({}),
    async execute(_toolCallId: string, _params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }] };
      }
      const mind = activeMind();
      const rendered = store.renderForInjection(mind, { showIds: true });
      const factCount = store.countActiveFacts(mind);
      return {
        content: [{ type: "text", text: rendered }],
        details: { facts: factCount, mind: activeLabel() },
      };
    },
  });

  pi.registerTool({
    name: "memory_recall",
    label: "Recall Memory",
    description: [
      "Semantically search project memory for facts relevant to a query.",
      "Returns ranked results by relevance × confidence — much more targeted than memory_query.",
      "Facts returned enter working memory and get priority in context injection.",
      "Falls back to keyword search (FTS5) if embedding models are unavailable.",
    ].join(" "),
    promptSnippet: "Semantic search over project memory — retrieve facts relevant to a query",
    promptGuidelines: [
      "Prefer memory_recall over memory_query for targeted retrieval — saves context tokens",
      "Recalled facts enter working memory and persist across compaction cycles",
    ],
    parameters: Type.Object({
      query: Type.String({ description: "Natural language query describing what you're looking for" }),
      k: Type.Optional(Type.Number({ description: "Number of results to return (default: 10, max: 30)" })),
      section: Type.Optional(StringEnum(
        ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions", "Specs"] as const,
        { description: "Optionally restrict search to a specific section" },
      )),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }], isError: true };
      }

      const mind = activeMind();
      const k = Math.min(params.k ?? 10, 30);

      // Try semantic search first
      if (embeddingAvailable) {
        const queryVec = await embedText(params.query);
        if (queryVec) {
          const results = store.semanticSearch(queryVec, mind, {
            k,
            minSimilarity: 0.25,
            section: params.section,
          });

          if (results.length > 0) {
            // Add to working memory
            addToWorkingMemory(...results.map(r => r.id));

            const lines = results.map((r, i) => {
              const sim = (r.similarity * 100).toFixed(0);
              const conf = (r.confidence * 100).toFixed(0);
              return `${i + 1}. [${r.id}] (${r.section}, ${sim}% match, ${conf}% conf) ${r.content}`;
            });

            // Also search episodes
            let episodeLines = "";
            const episodeVec = queryVec; // reuse
            const episodeHits = store.semanticSearchEpisodes(episodeVec, mind, { k: 3, minSimilarity: 0.35 });
            if (episodeHits.length > 0) {
              episodeLines = "\n\nRelated sessions:\n" + episodeHits.map(e =>
                `- ${e.date}: ${e.title} (${(e.similarity * 100).toFixed(0)}% match)\n  ${e.narrative.slice(0, 200)}${e.narrative.length > 200 ? "…" : ""}`
              ).join("\n");
            }

            return {
              content: [{ type: "text", text: lines.join("\n") + episodeLines }],
              details: {
                method: "semantic",
                results: results.length,
                workingMemorySize: workingMemory.size,
                episodes: episodeHits.length,
              },
            };
          }
        }
      }

      // Fallback: FTS5 keyword search on active facts
      const ftsResults = store.searchFacts(params.query, mind);
      const active = ftsResults.filter(f => f.status === "active");
      const limited = active.slice(0, k);

      if (limited.length === 0) {
        return { content: [{ type: "text", text: `No matching facts for: "${params.query}"` }] };
      }

      addToWorkingMemory(...limited.map(r => r.id));

      const lines = limited.map((r, i) =>
        `${i + 1}. [${r.id}] (${r.section}) ${r.content}`
      );

      return {
        content: [{ type: "text", text: lines.join("\n") }],
        details: { method: "fts5", results: limited.length, workingMemorySize: workingMemory.size },
      };
    },
  });

  pi.registerTool({
    name: "memory_episodes",
    label: "Session Episodes",
    description: [
      "Search session episode narratives — summaries of what happened in past work sessions.",
      "Episodes capture goals, decisions, sequences, and outcomes that individual facts don't preserve.",
      "Returns ranked results by semantic similarity to query.",
    ].join(" "),
    promptSnippet: "Search past session narratives for episodic context (goals, decisions, outcomes)",
    parameters: Type.Object({
      query: Type.String({ description: "What you're looking for in past sessions" }),
      k: Type.Optional(Type.Number({ description: "Number of results (default: 5)" })),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }], isError: true };
      }

      const mind = activeMind();
      const k = Math.min(params.k ?? 5, 15);

      // Try semantic search
      if (embeddingAvailable) {
        const queryVec = await embedText(params.query);
        if (queryVec) {
          const results = store.semanticSearchEpisodes(queryVec, mind, { k, minSimilarity: 0.25 });
          if (results.length > 0) {
            const lines = results.map((e, i) => {
              const sim = (e.similarity * 100).toFixed(0);
              const factIds = store!.getEpisodeFactIds(e.id);
              return [
                `${i + 1}. **${e.date}: ${e.title}** (${sim}% match)`,
                `   ${e.narrative}`,
                factIds.length > 0 ? `   Related facts: ${factIds.join(", ")}` : "",
              ].filter(Boolean).join("\n");
            });
            return {
              content: [{ type: "text", text: lines.join("\n\n") }],
              details: { method: "semantic", results: results.length },
            };
          }
        }
      }

      // Fallback: return most recent episodes
      const recent = store.getEpisodes(mind, k);
      if (recent.length === 0) {
        return { content: [{ type: "text", text: "No session episodes recorded yet." }] };
      }

      const lines = recent.map((e, i) =>
        `${i + 1}. **${e.date}: ${e.title}**\n   ${e.narrative}`
      );
      return {
        content: [{ type: "text", text: lines.join("\n\n") }],
        details: { method: "recent", results: recent.length },
      };
    },
  });

  pi.registerTool({
    name: "memory_focus",
    label: "Focus Working Memory",
    description: [
      "Pin specific facts into working memory so they persist across compaction.",
      "Working memory facts get priority injection in context. Use to keep important facts available",
      "throughout a long session without re-retrieving them. Call memory_release to clear.",
    ].join(" "),
    promptSnippet: "Pin facts to working memory (survives compaction, priority injection)",
    parameters: Type.Object({
      fact_ids: Type.Array(Type.String(), {
        description: "Fact IDs to pin (from memory_query or memory_recall output)",
        minItems: 1,
      }),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      addToWorkingMemory(...params.fact_ids);
      return {
        content: [{
          type: "text",
          text: `Pinned ${params.fact_ids.length} facts to working memory (${workingMemory.size}/${WORKING_MEMORY_CAP} slots used).`,
        }],
        details: { workingMemorySize: workingMemory.size },
      };
    },
  });

  pi.registerTool({
    name: "memory_release",
    label: "Release Working Memory",
    description: "Clear working memory buffer, releasing all pinned facts.",
    promptSnippet: "Clear working memory — release all pinned facts",
    parameters: Type.Object({}),
    async execute(_toolCallId: string, _params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      const released = workingMemory.size;
      workingMemory.clear();
      return {
        content: [{
          type: "text",
          text: `Released ${released} facts from working memory.`,
        }],
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
    promptSnippet: "Store a fact to project memory (persists across sessions)",
    promptGuidelines: [
      "Store conclusions, not investigation steps — if you're still debugging, don't store yet",
      "Store current state, not transitions — write 'X is used for Y', not 'X replaced Z for Y'",
      "Before storing, check if an existing fact covers it — use memory_supersede instead of adding duplicates",
      "After resolving a bug, archive all investigation breadcrumbs and store one decision fact about the fix",
      "Prefer pointer facts ('X does Y. See path/to/file.ts') over inlining implementation details",
    ],
    parameters: Type.Object({
      section: StringEnum(
        ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions", "Specs"] as const,
        { description: "Memory section to add the fact to" },
      ),
      content: Type.String({
        description: "Fact to add (single bullet point, self-contained)",
      }),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      if (!store) {
        return {
          content: [{ type: "text", text: "Project memory not initialized." }],
          isError: true,
        };
      }

      const mind = activeMind();
      const content = params.content.replace(/^-\s*/, "").trim();

      // Pre-store conflict detection: embed and check BEFORE committing
      let conflictWarning = "";
      let precomputedVec: Float32Array | null = null;
      if (embeddingAvailable) {
        precomputedVec = await embedText(`[${params.section}] ${content}`);
        if (precomputedVec) {
          const similar = store.findSimilarFacts(content, precomputedVec, mind, params.section, {
            threshold: 0.85,
            limit: 3,
          });
          if (similar.length > 0) {
            const warnings = similar.map(s =>
              `  ⚠ [${s.id}] (${(s.similarity * 100).toFixed(0)}% similar): ${s.content.slice(0, 100)}`
            );
            conflictWarning = "\n\nPotential conflicts detected — consider using memory_supersede if this replaces an existing fact:\n" + warnings.join("\n");
          }
        }
      }

      const result = store.storeFact({
        mind,
        section: params.section as any,
        content,
        source: "manual",
      });

      if (result.duplicate) {
        addToWorkingMemory(result.id);
        return {
          content: [{ type: "text", text: `Reinforced existing fact in ${params.section}: ${content}` }],
          details: { section: params.section, reinforced: true, id: result.id },
        };
      }

      // Store the precomputed vector directly (avoids redundant embedding call)
      if (precomputedVec && embeddingModel) {
        store.storeFactVector(result.id, precomputedVec, embeddingModel);
      } else {
        embedFact(result.id).catch(() => {}); // fallback fire-and-forget
      }

      addToWorkingMemory(result.id);
      return {
        content: [{ type: "text", text: `Stored in ${params.section}: ${content}${conflictWarning}` }],
        details: { section: params.section, id: result.id, facts: store.countActiveFacts(mind), conflicts: conflictWarning ? true : false },
      };
    },
  });

  pi.registerTool({
    name: "memory_supersede",
    label: "Supersede Memory Fact",
    description: [
      "Atomically replace an existing fact with a new version.",
      "The old fact is marked superseded (searchable in archive) and the new fact is stored.",
      "Ideal for updating specs, correcting facts, or evolving decisions.",
      "Get fact IDs from memory_query output (shown in [brackets]).",
    ].join(" "),
    promptSnippet: "Replace an existing fact with an updated version (atomic supersede)",
    parameters: Type.Object({
      fact_id: Type.String({ description: "ID of the fact to supersede" }),
      section: StringEnum(
        ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions", "Specs"] as const,
        { description: "Memory section for the new fact (can differ from original)" },
      ),
      content: Type.String({
        description: "New fact content (replaces the old fact)",
      }),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
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
        supersedes: params.fact_id,
      });

      return {
        content: [{ type: "text", text: `Superseded [${params.fact_id}] → new fact in ${params.section}: ${content}` }],
        details: { section: params.section, oldId: params.fact_id, newId: result.id, facts: store.countActiveFacts(mind) },
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
    promptSnippet: "Search archived memories from previous months",
    parameters: Type.Object({
      query: Type.String({ description: "Search terms (file paths, symbol names, concepts)" }),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
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
    promptSnippet: "Create a relationship between two facts in the knowledge graph",
    parameters: Type.Object({
      source_fact_id: Type.String({ description: "ID of the source fact" }),
      target_fact_id: Type.String({ description: "ID of the target fact" }),
      relation: Type.String({ description: "Short verb phrase: runs_on, depends_on, contradicts, etc." }),
      description: Type.String({ description: "Human-readable description of why these facts are connected" }),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
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

  pi.registerTool({
    name: "memory_archive",
    label: "Archive Memory Fact",
    description: [
      "Archive one or more facts from project memory by ID.",
      "Use to remove stale, redundant, or incorrect facts.",
      "Archived facts are searchable via memory_search_archive but no longer injected into context.",
      "Get fact IDs from memory_query output (shown in [brackets] when using the tool).",
    ].join(" "),
    promptSnippet: "Archive stale facts by ID (removes from active context, keeps in archive)",
    parameters: Type.Object({
      fact_ids: Type.Array(Type.String(), {
        description: "One or more fact IDs to archive",
        minItems: 1,
      }),
      reason: Type.Optional(Type.String({
        description: "Why these facts are being archived (logged, not shown to user)",
      })),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      if (!store) {
        return { content: [{ type: "text", text: "Project memory not initialized." }], isError: true };
      }

      const mind = activeMind();
      const results: string[] = [];
      let archived = 0;

      for (const id of params.fact_ids) {
        const fact = store.getFact(id);
        if (!fact) {
          results.push(`${id}: not found`);
          continue;
        }
        if (fact.status === "archived") {
          results.push(`${id}: already archived`);
          continue;
        }
        if (fact.mind !== mind) {
          results.push(`${id}: belongs to mind "${fact.mind}", not current mind "${mind}"`);
          continue;
        }
        store.archiveFact(id);
        archived++;
        results.push(`${id}: archived (was: ${fact.content.slice(0, 60)}…)`);
      }

      const remaining = store.countActiveFacts(mind);
      return {
        content: [{ type: "text", text: results.join("\n") }],
        details: { archived, remaining, reason: params.reason },
      };
    },
  });

  pi.registerTool({
    name: "memory_compact",
    label: "Compact Context",
    description: [
      "Trigger context compaction to free up context window space.",
      "Summarizes older conversation history, preserving recent work.",
      "After compaction, use memory_query to reload project knowledge into the fresh context.",
      "Use proactively when context is growing large, or after bulk archiving stale facts.",
      "The compaction runs asynchronously — the agent loop continues after it completes.",
    ].join(" "),
    promptSnippet: "Trigger context compaction to free context window space",
    promptGuidelines: [
      "Use proactively when context is growing large, or after bulk archiving stale facts",
    ],
    parameters: Type.Object({
      instructions: Type.Optional(Type.String({
        description: "Optional focus instructions for the compaction summary (e.g., 'preserve the architecture discussion')",
      })),
    }),
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, ctx: any): Promise<any> {
      const usage = ctx.getContextUsage();
      const pct = usage?.percent != null ? `${Math.round(usage.percent)}%` : "unknown";
      const tokens = usage?.tokens?.toLocaleString() ?? "unknown";

      // Resume is handled centrally in session_compact handler (covers all compaction sources).
      ctx.compact({
        customInstructions: params.instructions,
        onError: (err: Error) => {
          compactionRetryCount++;
          console.error(`[project-memory] Manual compaction failed: ${err.message}`);

          if (compactionRetryCount < config.compactionRetryLimit) {
            // Retry with local model
            useLocalCompaction = true;
            ctx.compact({
              customInstructions: params.instructions,
              onError: (retryErr: Error) => {
                console.error(`[project-memory] Local model compaction also failed: ${retryErr.message}`);
                if (ctx.hasUI) {
                  ctx.ui.notify("Compaction failed (cloud + local).", "error");
                }
              },
            });
          } else if (ctx.hasUI) {
            ctx.ui.notify("Compaction failed after max retries.", "error");
          }
        },
      });

      return {
        content: [{
          type: "text",
          text: [
            `Context compaction triggered (was ${pct} full, ${tokens} tokens).`,
            "Compaction runs in the background — older messages will be summarized.",
            "You will be prompted to continue after compaction completes.",
          ].join("\n"),
        }],
        details: { tokensBefore: usage?.tokens, percent: pct },
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
        ctx.ui.notify(`Switched to mind: ${mindName}`, "info");
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
        ctx.ui.notify(`Forked "${mindName}" → "${newName}"`, "info");
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
        const targetIndex = allMinds.findIndex(m => `${m.name} (${m.factCount} facts)` === targetIdx);
        if (targetIndex < 0) return;
        const target = allMinds[targetIndex];

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
          "info",
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
        const statusIdx = statuses.indexOf(idx as typeof statuses[number]);
        if (statusIdx < 0) return;
        store!.setMindStatus(mindName, statuses[statusIdx]);
        ctx.ui.notify(`Status of "${mindName}" → ${statuses[statusIdx]}`, "info");
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
        ctx.ui.notify(`Deleted mind: ${mindName}`, "info");
        break;
      }
    }
  }

  function updateStatus(ctx: ExtensionContext): void {
    if (!ctx.hasUI || !store) return;

    const theme = ctx.ui.theme;
    const mind = activeMind();
    const count = store.countActiveFacts(mind);

    // Label + fact count as a single unit: "Memory: 2 facts" or "Memory(mind): 2 facts"
    const label = mind !== "default" ? `Memory(${mind}): ${count} facts` : `Memory: ${count} facts`;
    const badges: string[] = [];

    // Working memory — pinned facts count
    if (workingMemory.size > 0) {
      badges.push(`${workingMemory.size} pinned`);
    }

    // Semantic search availability
    if (embeddingAvailable) {
      badges.push("semantic");
    }

    const status = badges.length > 0
      ? `${label} · ${badges.join(" · ")}`
      : label;

    ctx.ui.setStatus("memory", theme.fg("dim", status));
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
            ctx.ui.notify(`Archived ${count} facts`, "info");
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

          const vectorCount = store.countFactVectors(mind);
          const episodeCount = store.countEpisodes(mind);

          const lines = [
            `Mind: ${activeLabel()}`,
            `Active facts: ${total}`,
            `Embedded facts: ${vectorCount}/${total} (${total > 0 ? ((vectorCount / total) * 100).toFixed(0) : 0}%)`,
            `Episodes: ${episodeCount}`,
            `Working memory: ${workingMemory.size}/${WORKING_MEMORY_CAP}`,
            `Embedding model: ${embeddingAvailable ? embeddingModel : "unavailable"}`,
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
        ctx.ui.notify(`Created mind: ${name}`, "info");
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

  pi.registerCommand("exit", {
    description: "Run memory extraction and exit gracefully (avoids /reload terminal corruption)",
    handler: async (_args, ctx) => {
      if (!store) {
        ctx.shutdown();
        await new Promise<void>(resolve => {
          setTimeout(() => { resolve(); process.exit(0); }, 10_000);
        });
        return;
      }

      const mind = activeMind();
      const factsBefore = store.countActiveFacts(mind);

      // Run a final extraction if we have conversation context
      if (!triggerState.isRunning) {
        ctx.ui.notify("Running final memory extraction before exit…", "info");
        triggerState.isRunning = true;
        try {
          await runExtractionCycle(ctx, config);
        } catch {
          // Best effort — don't block exit
        } finally {
          triggerState.isRunning = false;
        }
      } else {
        // Wait for in-flight extraction to fully settle
        if (activeExtractionPromise) {
          ctx.ui.notify("Waiting for in-flight extraction…", "info");
          try { await activeExtractionPromise; } catch { /* killed or failed */ }
        }
      }

      const factsAfter = store.countActiveFacts(mind);
      const delta = factsAfter - factsBefore;

      // Generate session episode BEFORE goodbye (user sees progress, not post-goodbye lag)
      const branch = ctx.sessionManager.getBranch();
      const messages = branch
        .filter((e): e is SessionMessageEntry => e.type === "message")
        .map((e) => e.message);

      if (messages.length > 5) {
        ctx.ui.notify("Generating session summary…", "info");
        try {
          const recentMessages = messages.slice(-20);
          const serialized = serializeConversation(convertToLlm(recentMessages));

          // Try direct Ollama HTTP first (~500ms), fall back to subprocess (~3s)
          let episodeOutput = await generateEpisodeDirect(serialized, config);
          if (!episodeOutput) {
            episodeOutput = await generateEpisode(ctx.cwd, serialized, config);
          }

          if (episodeOutput && store) {
            const today = new Date().toISOString().split("T")[0];
            const sessionFactIds = [...workingMemory];
            const episodeId = store.storeEpisode({
              mind,
              title: episodeOutput.title,
              narrative: episodeOutput.narrative,
              date: today,
              factIds: sessionFactIds.filter(id => store!.getFact(id)?.status === "active"),
            });

            // Embed episode vector (non-blocking — runs during summary display)
            if (embeddingAvailable) {
              embedText(`${episodeOutput.title} ${episodeOutput.narrative}`)
                .then(vec => { if (vec && store) store.storeEpisodeVector(episodeId, vec, embeddingModel!); })
                .catch(() => {});
            }
          }
          exitEpisodeDone = true;
        } catch {
          // Best effort — don't block exit
        }
      } else {
        exitEpisodeDone = true;
      }

      // Build session-end summary from shared state
      const summaryLines: string[] = [];

      // Git state
      try {
        const branchResult = await pi.exec("git", ["branch", "--show-current"], { timeout: 3_000, cwd: ctx.cwd });
        const statusResult = await pi.exec("git", ["status", "--short"], { timeout: 3_000, cwd: ctx.cwd });
        const branchName = branchResult.stdout.trim();
        const dirtyCount = statusResult.stdout.trim().split("\n").filter(Boolean).length;
        summaryLines.push(`🔀 ${branchName}${dirtyCount > 0 ? ` · ${dirtyCount} dirty` : " · clean"}`);
      } catch { /* ignore */ }

      // Design tree
      const dt = sharedState.designTree;
      if (dt && dt.nodeCount > 0) {
        summaryLines.push(`🌳 Design: ${dt.nodeCount} nodes (${dt.decidedCount} decided, ${dt.exploringCount} exploring)`);
      }

      // OpenSpec
      const os = sharedState.openspec;
      if (os && os.changes.length > 0) {
        const active = os.changes.filter(c => c.stage !== "archived");
        if (active.length > 0) {
          summaryLines.push(`📋 OpenSpec: ${active.length} active — ${active.map(c => c.name).join(", ")}`);
        }
      }

      // Memory
      const memLine = delta > 0
        ? `🧠 ${factsAfter} facts (+${delta} new)`
        : `🧠 ${factsAfter} facts`;
      summaryLines.push(memLine);

      if (summaryLines.length > 0) {
        ctx.ui.notify(summaryLines.join("\n"), "info");
        await new Promise(r => setTimeout(r, 300));
      }

      ctx.ui.notify("Goodbye!", "info");

      // Small delay so the notification renders
      await new Promise(r => setTimeout(r, 200));

      // ctx.shutdown() is fire-and-forget internally (sets shutdownRequested flag
      // and calls void this.shutdown() in interactive mode). We must keep this
      // command handler alive so control doesn't return to the REPL prompt —
      // otherwise the user sees the input prompt again instead of the process exiting.
      ctx.shutdown();

      // Block until process.exit() is called by the shutdown flow.
      // The shutdown handler now only does JSONL export + DB close (fast),
      // since episode generation already completed above.
      await new Promise<void>(resolve => {
        setTimeout(() => {
          resolve();
          process.exit(0);
        }, 10_000);
      });
    },
  });
}
