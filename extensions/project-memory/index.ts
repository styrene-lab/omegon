/**
 * Project Memory Extension
 *
 * Persistent, cross-session project knowledge stored in SQLite with
 * confidence-decay reinforcement, semantic retrieval via cloud-first embeddings,
 * episodic session narratives, and working memory.
 *
 * Storage: .pi/memory/facts.db (SQLite with WAL mode)
 * Vectors: facts_vec / episodes_vec tables (Float32 BLOBs via configured embeddings)
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
 *   - Semantic retrieval via cloud-first embeddings (default: OpenAI text-embedding-3-small)
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
import type { ExtensionAPI, ExtensionContext, ExtensionCommandContext, SessionMessageEntry } from "@styrene-lab/pi-coding-agent";
import { DynamicBorder } from "@styrene-lab/pi-coding-agent";
import { sciCall, sciOk, sciErr, sciExpanded, sciLoading } from "./sci-renderers.ts";
import { sciExitCard, type ExitCardData } from "../lib/sci-ui.ts";
import { StringEnum } from "../lib/typebox-helpers";
import { Type } from "@sinclair/typebox";
import { Container, type SelectItem, SelectList, Text } from "@styrene-lab/pi-tui";
import { FactStore, parseExtractionOutput, GLOBAL_DECAY, type MindRecord, type Fact } from "./factstore.ts";
import { embed, isEmbeddingAvailable, resolveEmbeddingProvider, MODEL_DIMS, type EmbeddingProvider } from "./embeddings.ts";
import { DEFAULT_CONFIG, type MemoryConfig, type LifecycleMemoryCandidate } from "./types.ts";
import { sanitizeCompactionText, shouldInterceptCompaction } from "./compaction-policy.ts";
import { writeJsonlIfChanged } from "./jsonl-io.ts";
import {
  computeMemoryBudgetPolicy,
  createMemoryInjectionMetrics,
  estimateTokensFromChars,
  formatMemoryInjectionMetrics,
  type MemoryInjectionMode,
  type MemoryInjectionMetrics,
} from "./injection-metrics.ts";
import {
  type ExtractionTriggerState,
  createTriggerState,
  shouldExtract,
} from "./triggers.ts";
import { runExtractionV2, runGlobalExtraction, killActiveExtraction, killAllSubprocesses, generateEpisode, generateEpisodeDirect, generateEpisodeWithFallback, buildTemplateEpisode, runSectionPruningPass, type SessionTelemetry } from "./extraction-v2.ts";
import { migrateToFactStore, needsMigration, markMigrated } from "./migration.ts";
import { SECTIONS } from "./template.ts";
import { serializeConversation, convertToLlm } from "@styrene-lab/pi-coding-agent";
import { sharedState } from "../lib/shared-state.ts";
import {
  ingestLifecycleCandidate,
  ingestLifecycleCandidatesBatch,
  type LifecycleCandidate,
  type LifecycleCandidateResult,
  type BatchIngestResult,
} from "./lifecycle.ts";
import { 
  resolveTier, 
  getTierDisplayLabel, 
  getDefaultPolicy,
  getViableModels,
  type ModelTier, 
  type RegistryModel 
} from "../lib/model-routing.ts";

/** Map abstract effort model tiers to concrete cloud model IDs for extraction. */
const EFFORT_EXTRACTION_MODELS: Record<string, string> = {
  gloriana: "claude-opus-4-6",
  victory: "claude-sonnet-4-6",
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
// Canonical preference list + family prefix catch-alls from shared registry.
// Specific tags first (largest/best wins via startsWith); families catch any
// installed variant not explicitly listed (e.g. qwen3:14b-q4_k_m).
// Edit extensions/lib/local-models.ts to update model preferences.
import {
  PREFERRED_ORDER as LOCAL_MODELS_ORDER,
  PREFERRED_FAMILIES,
} from "../lib/local-models.ts";
const PREFERRED_CHAT_MODELS = [...LOCAL_MODELS_ORDER, ...PREFERRED_FAMILIES];

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
 * Resolve the compaction fallback chain based on effort tier and routing policy.
 * 
 * Returns an ordered array of { tier, timeout } objects representing the fallback chain.
 * Local models are resolved via discoverLocalChatModel(), cloud models via resolveTier().
 */
function resolveCompactionFallbackChain(
  ctx: ExtensionContext,
  config: MemoryConfig
): Array<{ tier: ModelTier; timeout: number; label: string }> {
  if (!config.compactionFallbackChain) {
    // Legacy behavior: only try local
    return [{ tier: "local", timeout: config.compactionLocalTimeout, label: "Local" }];
  }

  const effort = sharedState.effort;
  const policy = sharedState.routingPolicy ?? getDefaultPolicy();
  
  // Effort tiers 1-5: prefer local first. Tiers 6-7: can start with cloud.
  const startWithLocal = !effort || effort.compaction === "local";
  
  const chain: Array<{ tier: ModelTier; timeout: number; label: string }> = [];
  
  if (startWithLocal) {
    chain.push({ tier: "local", timeout: config.compactionLocalTimeout, label: "Local" });
  }
  
  // Add GPT-5.3-codex-spark (free reasoning model) as priority fallback
  chain.push({ tier: "victory", timeout: config.compactionCodexTimeout, label: "GPT-5.3-Codex-Spark" });
  
  // Add Haiku as budget fallback
  chain.push({ tier: "retribution", timeout: config.compactionHaikuTimeout, label: "Haiku" });
  
  // If we started with cloud, add local as final fallback
  if (!startWithLocal) {
    chain.push({ tier: "local", timeout: config.compactionLocalTimeout, label: "Local" });
  }
  
  return chain;
}

// tryCompactionWithTier will be defined after helper functions

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
  /** Pending embed promises — tracked so shutdown can await them before DB close */
  const pendingEmbeds = new Set<Promise<unknown>>();
  let consecutiveExtractionFailures = 0;
  let memoryDir = "";

  // --- Session Telemetry (for task-completion facts + template episode fallback) ---
  /** Files written this session (Write tool calls that succeeded) */
  const sessionFilesWritten: string[] = [];
  /** Files edited this session (Edit tool calls that succeeded) */
  const sessionFilesEdited: string[] = [];
  /** Pending write/edit args, keyed by toolCallId, collected from tool_call events */
  const pendingWriteEditArgs = new Map<string, { toolName: string; path: string }>();
  /** Proactive startup payload — injected on firstTurn before semantic retrieval */
  let startupInjectionPayload: string | null = null;
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

  // --- Injection Calibration State ---
  let pendingInjectionCalibration: {
    baselineContextTokens: number | null;
    userPromptTokensEstimate: number;
  } | null = null;

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

  function getEmbeddingOpts(): { provider: EmbeddingProvider; model: string } | null {
    if (!embeddingModel) return null;
    return {
      provider: config.embeddingProvider,
      model: embeddingModel,
    };
  }

  /**
   * Fire-and-forget embed with tracking. The promise is added to pendingEmbeds
   * and auto-removed on completion. Shutdown awaits all pending before DB close.
   */
  function trackEmbed(p: Promise<unknown>): void {
    pendingEmbeds.add(p);
    p.finally(() => pendingEmbeds.delete(p));
  }

  /** Embed a single text, returning the vector or null if unavailable */
  async function embedText(text: string): Promise<Float32Array | null> {
    if (!embeddingAvailable) return null;
    const opts = getEmbeddingOpts();
    if (!opts) return null;
    const result = await embed(text, opts);
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
    const opts = getEmbeddingOpts();
    if (!opts) return false;
    const result = await embed(
      `[${fact.section}] ${fact.content}`,
      opts,
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
    const totalActive = store.countActiveFacts(mind);
    const missing = store.getFactsMissingVectors(mind);

    // Health check: warn if coverage has degraded significantly
    if (totalActive > 0) {
      const coverage = 1 - missing.length / totalActive;
      if (coverage < 0.5) {
        console.error(`[project-memory] WARNING: vector coverage critically low: ${Math.round(coverage * 100)}% (${missing.length}/${totalActive} facts missing vectors)`);
      } else if (coverage < 0.9 && missing.length > 10) {
        console.warn(`[project-memory] vector coverage: ${Math.round(coverage * 100)}% — indexing ${missing.length} facts`);
      }
    }

    if (missing.length === 0) return;

    let indexed = 0;
    let failed = 0;
    let consecutiveFailures = 0;
    for (const factId of missing) {
      if (!sessionActive) break; // Stop if session is shutting down
      const ok = await embedFact(factId);
      if (ok) {
        indexed++;
        consecutiveFailures = 0;
      } else {
        failed++;
        consecutiveFailures++;
        // If 5 consecutive failures, the embedding provider is likely down.
        // Stop early to avoid burning time on a dead service.
        if (consecutiveFailures >= 5) {
          console.error(`[project-memory] embedding indexer: 5 consecutive failures, stopping early (indexed ${indexed}, failed ${failed} of ${missing.length})`);
          break;
        }
      }
    }

    if (indexed > 0 || failed > 0) {
      const finalVecs = store.countFactVectors(mind);
      const finalCoverage = totalActive > 0 ? Math.round((finalVecs / totalActive) * 100) : 100;
      if (ctx.hasUI) {
        if (failed > 0 && consecutiveFailures >= 5) {
          ctx.ui.notify(
            `Embedding indexer stopped: ${indexed} indexed, ${failed} failed (provider may be down). Coverage: ${finalCoverage}%`,
            "warning",
          );
        } else if (indexed > 5) {
          // Only notify if we indexed a meaningful batch — don't spam for 1-2 new facts
          ctx.ui.notify(
            `Indexed ${indexed} facts for semantic search (${finalCoverage}% coverage)`,
            "info",
          );
        }
      }
      if (failed > 0) {
        console.warn(`[project-memory] background indexing: ${indexed} indexed, ${failed} failed, coverage ${finalCoverage}%`);
      }
    }

    // Also index global store facts
    if (globalStore) {
      const globalMind = globalStore.getActiveMind() ?? "default";
      const globalMissing = globalStore.getFactsMissingVectors(globalMind);
      for (const factId of globalMissing) {
        if (!sessionActive) break;
        const fact = globalStore.getFact(factId);
        if (!fact || fact.status !== "active") continue;
        const opts = getEmbeddingOpts();
        if (!opts) continue;
        const result = await embed(
          `[${fact.section}] ${fact.content}`,
          opts,
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
    drainLifecycleCandidateQueue(ctx);
    drainFactArchiveQueue();
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
        ? "\nFix: run `npm rebuild better-sqlite3` in the Omegon directory, then restart."
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
        ? "\nFix: run `npm rebuild better-sqlite3` in the Omegon directory, then restart."
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
    sessionFilesWritten.length = 0;
    sessionFilesEdited.length = 0;
    pendingWriteEditArgs.clear();
    startupInjectionPayload = null;

    // Apply effort-tier overrides to extraction and compaction config.
    // sharedState.effort is written by the effort extension's session_start,
    // which fires before ours (effort is registered earlier in package.json).
    config = { ...DEFAULT_CONFIG };

    // Auto-detect embedding provider (Custom > Voyage > OpenAI > Ollama > FTS5 fallback)
    // Auto-detect embedding provider from env vars or local inference.
    // MEMORY_EMBEDDING_PROVIDER can override if user wants to force a specific provider.
    const envEmbeddingProvider = process.env.MEMORY_EMBEDDING_PROVIDER as EmbeddingProvider | undefined;
    if (envEmbeddingProvider === "voyage" || envEmbeddingProvider === "openai" || envEmbeddingProvider === "openai-compatible" || envEmbeddingProvider === "ollama") {
      config.embeddingProvider = envEmbeddingProvider;
      // When provider is explicitly set but model isn't, use provider-appropriate defaults
      // instead of keeping DEFAULT_CONFIG's voyage model for a non-voyage provider.
      if (!process.env.MEMORY_EMBEDDING_MODEL) {
        const providerDefaults: Record<string, string> = {
          voyage: "voyage-3-lite",
          openai: "text-embedding-3-small",
          ollama: "qwen3-embedding:0.6b",
          "openai-compatible": "text-embedding-3-small",
        };
        config.embeddingModel = providerDefaults[envEmbeddingProvider] ?? config.embeddingModel;
      }
    } else {
      // Auto-detect: resolveEmbeddingProvider checks env vars and falls back to Ollama.
      // Always returns a candidate (Ollama is the unconditional fallback);
      // isEmbeddingAvailable() validates below with a real healthcheck request.
      const detected = resolveEmbeddingProvider();
      if (detected) {
        config.embeddingProvider = detected.provider;
        config.embeddingModel = detected.model;
      }
    }
    const envEmbeddingModel = process.env.MEMORY_EMBEDDING_MODEL?.trim();
    if (envEmbeddingModel) config.embeddingModel = envEmbeddingModel;
    const envExtractionModel = process.env.MEMORY_EXTRACTION_MODEL?.trim();
    if (envExtractionModel) config.extractionModel = envExtractionModel;

    const effort = sharedState.effort;
    if (effort) {
      // Extraction: tiers 1-5 use local (devstral default), tiers 6-7 use cloud
      if (effort.extraction !== "local") {
        const model = EFFORT_EXTRACTION_MODELS[effort.extraction];
        if (model) config.extractionModel = model;
      }
      // Compaction: only explicitly local tiers intercept before pi core.
      // Normal day-to-day cloud-backed tiers defer to provider-routed compaction first.
      config.compactionLocalFirst = effort.compaction === "local";
    }

    // Detect embedding availability and start background indexing
    try {
      const embedStatus = await isEmbeddingAvailable({
        provider: config.embeddingProvider,
        model: config.embeddingModel,
      });
      embeddingAvailable = embedStatus.available;
      embeddingModel = embedStatus.model;
      if (embeddingAvailable && embeddingModel) {
        // Purge vectors from a different model (dimension mismatch)
        const expectedDims = embedStatus.dims ?? MODEL_DIMS[embeddingModel];
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
        backgroundIndexFacts(ctx).catch((err) => {
          console.error(`[project-memory] background indexer error:`, err?.message ?? err);
        });
      } else if (ctx.hasUI) {
        // Tell the user semantic search is unavailable so they know what to expect.
        // Common causes: Ollama not running, embedding model not pulled, no cloud API key.
        const providerHint = config.embeddingProvider === "ollama"
          ? " (is Ollama running? try: ollama pull qwen3-embedding:0.6b)"
          : ` (${config.embeddingProvider} — check API key)`;
        ctx.ui.notify(
          `Semantic search unavailable${providerHint} — using keyword search`,
          "warning",
        );
      }
    } catch {
      embeddingAvailable = false;
    }

    // --- Decay sweep + per-section pruning (background, non-blocking) ---
    // 1. Archive facts that have decayed below their profile's minimum confidence.
    //    Converts passive decay (lower score on read) into active archival.
    //    Without this, decayed facts accumulate forever as active but invisible.
    // 2. When any section exceeds 60 facts, run a targeted LLM archival pass
    //    to bring it back under the ceiling. This prevents monotonic accumulation.
    // Runs fire-and-forget so it doesn't block session startup.
    if (store) {
      (async () => {
        try {
          const mind = activeMind();

          // Phase 1: Sweep decayed facts (deterministic, no LLM needed)
          const swept = store!.sweepDecayedFacts(mind);
          if (swept > 0 && ctx.hasUI) {
            ctx.ui.notify(`Archived ${swept} decayed facts`, "info");
          }

          // Phase 2: Section ceiling pruning (LLM-assisted)
          const SECTION_CEILING = 60;
          const sectionCounts = store!.getSectionCounts(mind);
          for (const [section, count] of sectionCounts) {
            if (count <= SECTION_CEILING) continue;
            // Skip Recent Work — decay sweep handles it (fast decay, no LLM needed)
            if (section === "Recent Work") continue;

            // Single query — all subsequent phases operate on this in-memory list.
            // getFactsBySection already computes confidence via computeConfidence
            // (canonical formula from core.ts), so sorted.confidence is correct.
            const facts = store!.getFactsBySection(mind, section);
            const excess = count - SECTION_CEILING;
            let archived = 0;
            const archivedIds = new Set<string>();

            // Phase 2a: Deterministic cull — archive only facts with confidence
            // below a hard floor. This protects semantically important facts that
            // happen to have low confidence purely due to age. Facts above the
            // floor are sent to the LLM for judgment (phase 2b).
            const CONFIDENCE_FLOOR = 0.25;
            const sorted = [...facts].sort((a, b) => a.confidence - b.confidence);
            for (const f of sorted) {
              if (archived >= excess) break;        // enough culled
              if (f.confidence > CONFIDENCE_FLOOR) break; // above floor → LLM decides
              store!.archiveFact(f.id);
              archivedIds.add(f.id);
              archived++;
            }

            // Phase 2b: LLM-assisted pruning for the remaining excess.
            // Filter the in-memory array (no re-query) and send a manageable
            // batch (up to 80 facts) to the LLM for nuanced review.
            const remaining = facts.filter(f => !archivedIds.has(f.id));
            const stillExcess = remaining.length - SECTION_CEILING;
            if (stillExcess > 0) {
              // Send the bottom 80 by confidence for LLM review
              const batch = [...remaining]
                .sort((a, b) => a.confidence - b.confidence)
                .slice(0, Math.min(80, remaining.length));
              const idsToArchive = await runSectionPruningPass(section, batch, SECTION_CEILING, config);
              const validIds = new Set(batch.map(f => f.id));
              const safeToArchive = idsToArchive.filter(id => validIds.has(id));
              for (const id of safeToArchive) {
                store!.archiveFact(id);
                archived++;
              }
            }

            if (archived > 0 && ctx.hasUI) {
              ctx.ui.notify(
                `Memory pruned ${archived} facts from ${section} (was ${count}, ceiling ${SECTION_CEILING})`,
                "info",
              );
            }
          }
        } catch {
          // Best effort — don't interrupt session
        }
      })();
    }

    // --- Proactive startup injection (session-continuity) ---
    // Inject three layers before the user's first message so continuation
    // questions work without waiting for semantic retrieval.
    // Layer 1: last 1 session episode (most recent — use memory_episodes for more)
    // Layer 2: top-15 recently-reinforced facts (recency window, cross-section)
    // Layer 3: Decisions + Constraints + Known Issues always; Architecture capped at 10
    //
    // Architecture is the largest section by fact count. Loading all Architecture
    // facts unconditionally blows context on large projects. The top-10 by recency
    // covers active concerns; older facts are retrievable via memory_recall.
    //
    // This runs asynchronously so it doesn't block the TUI from appearing.
    // The payload is injected as a pre-prompt system message on the first turn.
    if (store) {
      try {
        const mind = activeMind();
        const recentEpisodes = store.getEpisodes(mind, 1);
        const allFacts = store.getActiveFacts(mind);

        // Recency window: top 15 by last_reinforced (any section)
        const recentFacts = [...allFacts]
          .sort((a, b) => new Date(b.last_reinforced).getTime() - new Date(a.last_reinforced).getTime())
          .slice(0, 15);

        // Core structural facts: Decisions + Constraints + Known Issues always loaded.
        // Architecture capped at 10 most-recently-reinforced (largest section by volume).
        const coreFacts = allFacts.filter(f =>
          f.section === "Decisions" || f.section === "Constraints" || f.section === "Known Issues"
        );
        const archFacts = [...allFacts]
          .filter(f => f.section === "Architecture")
          .sort((a, b) => new Date(b.last_reinforced).getTime() - new Date(a.last_reinforced).getTime())
          .slice(0, 10);

        // Merge: recent episodes + recency window + core sections (deduplicated)
        // Budget-capped to prevent context overflow on large fact stores.
        const STARTUP_MAX_CHARS = 12_000; // ~3K tokens — leaves room for system prompt + design-tree
        let startupChars = 0;
        const startupFactIds = new Set<string>();
        const startupFacts: typeof allFacts = [];
        for (const f of [...coreFacts, ...archFacts, ...recentFacts]) {
          if (startupFactIds.has(f.id)) continue;
          const cost = f.content.length + 20;
          if (startupChars + cost > STARTUP_MAX_CHARS) break;
          startupFacts.push(f);
          startupFactIds.add(f.id);
          startupChars += cost;
        }

        if (recentEpisodes.length > 0 || startupFacts.length > 0) {
          const lines: string[] = ["<!-- Startup Context — recent sessions and structural memory -->", ""];

          if (recentEpisodes.length > 0) {
            lines.push("## Recent Sessions");
            lines.push("_Episodic memory — what happened and why_");
            lines.push("");
            for (const ep of recentEpisodes) {
              lines.push(`### ${ep.date}: ${ep.title}`);
              lines.push(ep.narrative);
              lines.push("");
            }
          }

          if (startupFacts.length > 0) {
            const factsBySection = new Map<string, typeof startupFacts>();
            for (const f of startupFacts) {
              const sec = factsBySection.get(f.section) ?? [];
              sec.push(f);
              factsBySection.set(f.section, sec);
            }
            for (const [section, facts] of factsBySection) {
              lines.push(`## ${section}`);
              lines.push("");
              for (const f of facts) {
                lines.push(`- ${f.content}`);
              }
              lines.push("");
            }
          }

          startupInjectionPayload = lines.join("\n");
        }
      } catch {
        // Best effort — don't block startup
      }
    }

    updateStatus(ctx);
  });

  // ---------------------------------------------------------------------------
  // Compaction fallback chain helpers (require store to be initialized)
  // ---------------------------------------------------------------------------

  /**
   * Try local compaction using the existing Ollama path.
   */
  async function tryLocalCompaction(
    localModel: string, 
    prep: any, 
    customInstructions: string | undefined,
    signal: AbortSignal
  ): Promise<{ summary: string; details: any } | null> {
    // Build summarization prompt (same as existing logic)
    const llmMessages = convertToLlm(prep.messagesToSummarize);
    let conversationText = sanitizeCompactionText(serializeConversation(llmMessages));

    // Truncate to ~60k chars (~15k tokens) to fit local model context windows
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

    const basePrompt = prep.previousSummary ? COMPACTION_UPDATE_PROMPT : COMPACTION_INITIAL_PROMPT;
    promptText += customInstructions ? `${basePrompt}\n\nAdditional focus: ${customInstructions}` : basePrompt;

    // Handle split turn prefix if needed
    let turnPrefixSummary = "";
    if (prep.isSplitTurn && prep.turnPrefixMessages.length > 0) {
      const prefixMessages = convertToLlm(prep.turnPrefixMessages);
      let prefixText = sanitizeCompactionText(serializeConversation(prefixMessages));
      if (prefixText.length > MAX_CONVERSATION_CHARS) {
        prefixText = "...[truncated]...\n\n" + prefixText.slice(-MAX_CONVERSATION_CHARS);
      }
      const prefixPrompt = `<conversation>\n${prefixText}\n</conversation>\n\n${COMPACTION_TURN_PREFIX_PROMPT}`;

      try {
        const prefixResp = await ollamaChat(localModel, COMPACTION_SYSTEM_PROMPT, prefixPrompt, {
          maxTokens: 2048, signal,
        });
        if (prefixResp) turnPrefixSummary = prefixResp;
      } catch {
        // If turn prefix fails, continue without it
      }
    }

    // Generate main summary
    const summary = await ollamaChat(localModel, COMPACTION_SYSTEM_PROMPT, promptText, {
      maxTokens: 4096, signal,
    });

    if (!summary) return null;

    let fullSummary = summary;
    if (turnPrefixSummary) {
      fullSummary += `\n\n---\n\n**Turn Context (split turn):**\n\n${turnPrefixSummary}`;
    }

    // Append file operations
    fullSummary += formatFileOps(prep.fileOps);

    return {
      summary: fullSummary,
      details: buildFileDetails(prep.fileOps),
    };
  }

  /**
   * Try cloud compaction by falling through to pi's core compaction.
   * This is a placeholder - we don't duplicate pi's cloud calling logic here.
   */
  async function tryCloudCompaction(
    model: any,
    prep: any, 
    customInstructions: string | undefined,
    signal: AbortSignal,
    ctx: ExtensionContext
  ): Promise<{ summary: string; details: any } | null> {
    // We deliberately return null here to fall through to pi's core compaction
    // which already has the cloud model calling infrastructure.
    // This allows us to leverage pi's existing robust cloud compaction 
    // while controlling which model gets selected via the fallback chain.
    console.log(`[project-memory] Falling through to pi core compaction with ${model.id}`);
    return null;
  }

  /**
   * Attempt compaction with a specific model tier from the fallback chain.
   * Returns the compaction result or null if this tier failed.
   */
  async function tryCompactionWithTier(
    tier: ModelTier,
    timeout: number,
    label: string,
    prep: any,
    customInstructions: string | undefined,
    signal: AbortSignal,
    ctx: ExtensionContext
  ): Promise<{ summary: string; details: any } | null> {
    try {
      const timeoutSignal = AbortSignal.timeout(timeout);
      const combinedSignal = AbortSignal.any([signal, timeoutSignal]);
      
      if (tier === "local") {
        // Use local Ollama path
        const localModel = await discoverLocalChatModel();
        if (!localModel) {
          console.log(`[project-memory] ${label} model not available`);
          return null;
        }
        
        return await tryLocalCompaction(localModel, prep, customInstructions, combinedSignal);
      } else {
        // Use cloud model via model registry
        const all = getViableModels(ctx.modelRegistry);
        const policy = sharedState.routingPolicy ?? getDefaultPolicy();
        const resolved = resolveTier(tier, all, policy);
        
        if (!resolved) {
          console.log(`[project-memory] No ${label} model available via provider routing`);
          return null;
        }
        
        const model = all.find((m) => m.id === resolved.modelId);
        if (!model) {
          console.log(`[project-memory] ${label} model ${resolved.modelId} not found in registry`);
          return null;
        }
        
        return await tryCloudCompaction(model as any, prep, customInstructions, combinedSignal, ctx);
      }
    } catch (err) {
      if (signal.aborted) throw err; // Don't swallow user cancellation
      console.log(`[project-memory] ${label} compaction failed: ${err instanceof Error ? err.message : String(err)}`);
      return null;
    }
  }

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
    // For non-/exit shutdowns (ctrl-c, /reload), use the fallback chain so we
    // always emit at least a template episode. Guaranteed, never null.
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
          const today = new Date().toISOString().split("T")[0];

          const telemetry: SessionTelemetry = {
            date: today,
            toolCallCount: triggerState.toolCallsSinceExtract,
            filesWritten: [...sessionFilesWritten],
            filesEdited: [...sessionFilesEdited],
          };

          // Use fallback chain — always returns an episode (template at worst)
          const episodeOutput = await generateEpisodeWithFallback(serialized, telemetry, config, ctx.cwd);
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
      } catch {
        // Best effort — don't block shutdown
      }
    }

    // Drain pending embed promises before JSONL export + DB close.
    // These are fire-and-forget embedFact/embedText calls from extraction,
    // memory_store, and /exit episode generation. Timeout after 5s to avoid
    // blocking shutdown if Ollama is hung.
    if (pendingEmbeds.size > 0) {
      try {
        await Promise.race([
          Promise.allSettled([...pendingEmbeds]),
          new Promise(r => setTimeout(r, 5_000)),
        ]);
      } catch { /* best effort */ }
    }

    // JSONL export + DB close (fast — synchronous I/O, ~50ms)
    if (store) {
      try {
        const fsSync = await import("node:fs");
        const jsonlPath = path.join(memoryDir, "facts.jsonl");
        const jsonl = store.exportToJsonl();
        writeJsonlIfChanged(fsSync, jsonlPath, jsonl);
      } catch {
        // Best effort — don't block shutdown
      }
    }

    store?.close(); globalStore?.close();
  });

  // --- Local-model compaction ---
  // Two modes:
  // 1. compactionLocalFirst=true: intercept ALL compactions, try local first.
  // 2. compactionLocalFirst=false (default): only intercept when useLocalCompaction is set
  //    after a cloud failure or explicit local policy choice.
  pi.on("session_before_compact", async (event, ctx) => {
    if (!shouldInterceptCompaction(sharedState.effort?.compaction, config, useLocalCompaction)) return;
    useLocalCompaction = false; // consume the flag if it was set

    const prep = event.preparation;
    if (!prep || prep.messagesToSummarize.length === 0) return;

    // Get the intelligent fallback chain
    const fallbackChain = resolveCompactionFallbackChain(ctx, config);
    
    if (ctx.hasUI) {
      const isRetry = !config.compactionLocalFirst;
      const firstTier = fallbackChain[0]?.label || "Unknown";
      ctx.ui.notify(
        isRetry
          ? `Cloud compaction failed — trying intelligent fallback: ${firstTier}`
          : `Intelligent compaction: ${firstTier} → GPT-5.3 → Haiku fallback chain`,
        isRetry ? "warning" : "info",
      );
    }

    // Try each tier in the fallback chain
    for (const [index, tier] of fallbackChain.entries()) {
      console.log(`[project-memory] Trying compaction tier ${index + 1}/${fallbackChain.length}: ${tier.label} (${tier.timeout}ms timeout)`);
      
      const result = await tryCompactionWithTier(
        tier.tier,
        tier.timeout,
        tier.label,
        prep,
        event.customInstructions,
        event.signal,
        ctx
      );
      
      if (result) {
        console.log(`[project-memory] Compaction succeeded with ${tier.label}`);
        return {
          compaction: {
            summary: result.summary,
            firstKeptEntryId: prep.firstKeptEntryId,
            tokensBefore: prep.tokensBefore,
            details: result.details,
          },
        };
      }
      
      // If this was a cloud tier that returned null, it means we should fall through
      // to pi's core compaction with that model instead of continuing the chain.
      if (tier.tier !== "local" && result === null) {
        console.log(`[project-memory] ${tier.label} requesting fallthrough to pi core compaction`);
        return; // Let pi core handle it with the selected model
      }
      
      console.log(`[project-memory] ${tier.label} compaction failed, trying next tier`);
    }

    // All tiers in the chain failed
    console.error(`[project-memory] All compaction tiers failed. Fallback chain: ${fallbackChain.map(t => t.label).join(" → ")}`);
    return; // Let pi core attempt compaction as final fallback
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

      // Embed newly created facts (tracked fire-and-forget — shutdown awaits these)
      if (result.newFactIds.length > 0 && embeddingAvailable) {
        for (const id of result.newFactIds) {
          trackEmbed(embedFact(id).catch(() => {}));
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
    drainLifecycleCandidateQueue(ctx);
    drainFactArchiveQueue();
    if (!store) return;
    if (!firstTurn && !postCompaction) return;

    firstTurn = false;
    postCompaction = false;

    const mind = activeMind();
    const factCount = store.countActiveFacts(mind);
    const mindLabel = mind !== "default" ? ` (mind: ${mind})` : "";

    if (factCount <= 3) {
      const content = [
        `Project memory initialized${mindLabel} (${factCount} facts stored).`,
        "Use **memory_store** to persist important discoveries as you work",
        "(architecture decisions, constraints, patterns, known issues).",
        "Facts persist across sessions and will be available next time.",
      ].join(" ");
      const usage = ctx.getContextUsage();
      const metrics = createMemoryInjectionMetrics({
        mode: "tiny",
        projectFactCount: factCount,
        payloadChars: content.length,
        baselineContextTokens: usage?.tokens ?? null,
        userPromptTokensEstimate: estimateTokensFromChars((event as any).prompt ?? ""),
      });
      sharedState.memoryTokenEstimate = metrics.estimatedTokens;
      sharedState.lastMemoryInjection = metrics;
      pendingInjectionCalibration = {
        baselineContextTokens: metrics.baselineContextTokens ?? null,
        userPromptTokensEstimate: metrics.userPromptTokensEstimate ?? 0,
      };
      return {
        message: {
          customType: "project-memory",
          content,
          display: false,
        },
      };
    }

    // --- Unified Priority-Ordered Context Pipeline ---
    //
    // Single pipeline replaces the old dual bulk/semantic split. Facts are
    // selected in priority order and rendered within a character budget.
    // When embeddings are unavailable, tiers 4a (FTS5) and 5/6 naturally
    // fill the budget — no separate codepath needed.
    //
    // Priority tiers:
    //   1. Working memory (pinned facts) — highest priority
    //   2. Decisions (top N by confidence) — structural anchor
    //   3. Architecture (top N by recency) — structural anchor
    //   4. Hybrid search (FTS5 + embedding via RRF) — query-relevant
    //   5. Structural fill (top 2 per remaining section) — coverage
    //   6. Recency fill (most recently reinforced) — fills remaining budget

    const userText = (event as any).prompt ?? "";
    const usage = ctx.getContextUsage();

    // Budget: reserve space for other context (design-tree, system prompt, etc.)
    // Routine turns should carry a much smaller memory payload by default.
    const budgetPolicy = computeMemoryBudgetPolicy({
      usedTokens: usage?.tokens,
      usedPercent: usage?.percent,
      userText,
    });
    const MAX_MEMORY_CHARS = budgetPolicy.maxChars;

    const allFacts = store.getActiveFacts(mind);
    const injectedIds = new Set<string>();
    const injectedFacts: Fact[] = [];
    let currentChars = 0;
    let injectedWorkingMemoryFactCount = 0;
    let injectedSemanticHitCount = 0;

    /** Measure a fact's rendered line length */
    function factCharCost(f: Fact): number {
      return f.content.length + 20; // bullet + date + newline overhead
    }

    /** Add a fact if it fits the budget and isn't already included */
    function tryAdd(f: Fact): boolean {
      if (injectedIds.has(f.id)) return false;
      const cost = factCharCost(f);
      if (currentChars + cost > MAX_MEMORY_CHARS) return false;
      injectedFacts.push(f);
      injectedIds.add(f.id);
      currentChars += cost;
      return true;
    }

    // --- Tier 1: Working memory (pinned facts) — always included first ---
    const wmFacts = [...workingMemory]
      .map(id => store!.getFact(id))
      .filter((f): f is Fact => f !== null && f.status === "active");
    for (const f of wmFacts) {
      tryAdd(f);
      injectedWorkingMemoryFactCount++;
    }

    // --- Tier 2: Decisions (top 8 by confidence) — structural anchor ---
    const decisionFacts = allFacts
      .filter(f => f.section === "Decisions")
      .sort((a, b) => b.confidence - a.confidence)
      .slice(0, 8);
    for (const f of decisionFacts) tryAdd(f);

    // --- Tier 3: Architecture (top 8 by recency) — structural anchor ---
    const archFacts = allFacts
      .filter(f => f.section === "Architecture")
      .sort((a, b) => new Date(b.last_reinforced).getTime() - new Date(a.last_reinforced).getTime())
      .slice(0, 8);
    for (const f of archFacts) tryAdd(f);

    // --- Tier 4: Hybrid search (FTS5 + embedding via RRF) ---
    // Uses hybridSearch which combines FTS5 keyword matching with semantic
    // embedding search. When embeddings are unavailable, degrades to FTS5-only.
    if (userText.length > 5 && currentChars < MAX_MEMORY_CHARS) {
      const queryVec = await embedText(userText);
      const hybridHits = store.hybridSearch(userText, queryVec, mind, {
        k: 15,
        minSimilarity: 0.3,
      });
      for (const f of hybridHits) {
        if (!tryAdd(f)) break; // budget exhausted
        injectedSemanticHitCount++;
      }
    }

    // --- Tier 5: Structural fill — top 2 per remaining section ---
    // Ensures every section has representation even without a matching query.
    // This is what bulk mode got right that old semantic mode missed.
    const FILL_SECTIONS = ["Constraints", "Known Issues", "Patterns & Conventions", "Specs", "Recent Work"] as const;
    if (budgetPolicy.includeStructuralFill) {
      for (const section of FILL_SECTIONS) {
        if (currentChars >= MAX_MEMORY_CHARS) break;
        const sectionFacts = allFacts
          .filter(f => f.section === section)
          .sort((a, b) => b.confidence - a.confidence)
          .slice(0, 2);
        for (const f of sectionFacts) {
          if (!tryAdd(f)) break;
        }
      }
    }

    // --- Tier 6: Recency fill — most recently reinforced not yet included ---
    if (budgetPolicy.includeStructuralFill && currentChars < MAX_MEMORY_CHARS) {
      const recentFacts = [...allFacts]
        .sort((a, b) => new Date(b.last_reinforced).getTime() - new Date(a.last_reinforced).getTime())
        .slice(0, 12);
      for (const f of recentFacts) {
        if (!tryAdd(f)) break;
      }
    }

    const injectedProjectFactCount = injectedFacts.length;
    const rendered = store.renderFactList(injectedFacts, { showIds: false });

    // --- Global knowledge: semantic-gated, only when query is available ---
    let globalSection = "";
    let injectedGlobalFactCount = 0;
    if (budgetPolicy.includeGlobalFacts && globalStore && userText.length > 10) {
      const globalMind = globalStore.getActiveMind() ?? "default";
      const globalFactCount = globalStore.countActiveFacts(globalMind);
      if (globalFactCount > 0) {
        const queryVec = await embedText(userText);
        if (queryVec) {
          const globalHits = globalStore.semanticSearch(queryVec, globalMind, { k: 6, minSimilarity: 0.45 });
          if (globalHits.length > 0) {
            injectedGlobalFactCount = globalHits.length;
            const globalRendered = globalStore.renderFactList(globalHits, { showIds: false });
            globalSection = `\n\n<!-- Global Knowledge — cross-project facts and connections -->\n${globalRendered}`;
          }
        } else {
          // Embeddings unavailable — use FTS5 for global search
          const globalHits = globalStore.searchFacts(userText, globalMind).slice(0, 4);
          if (globalHits.length > 0) {
            injectedGlobalFactCount = globalHits.length;
            const globalRendered = globalStore.renderFactList(globalHits, { showIds: false });
            globalSection = `\n\n<!-- Global Knowledge — cross-project facts and connections -->\n${globalRendered}`;
          }
        }
      }
    }

    // --- Episodes: 1 most recent ---
    let episodeSection = "";
    let injectedEpisodeCount = 0;
    const episodeCount = store.countEpisodes(mind);
    if (budgetPolicy.includeEpisode && episodeCount > 0) {
      const recentEpisodes = store.getEpisodes(mind, 1);
      if (recentEpisodes.length > 0) {
        injectedEpisodeCount = recentEpisodes.length;
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

    // Context pressure — continuous gradient from onset through warning
    let pressureWarning = "";
    if (!autoCompacted) {
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

    // Proactive startup payload — prepend on firstTurn if available.
    const startupSection = startupInjectionPayload
      ? `\n\n${startupInjectionPayload}`
      : "";
    startupInjectionPayload = null; // consume once

    const budgetNote = currentChars >= MAX_MEMORY_CHARS
      ? ` (budget-capped at ~${Math.round(MAX_MEMORY_CHARS / 1000)}K chars)`
      : "";

    const injectionContent = [
      `Project memory available${mindLabel} (${factCount} facts from this and previous sessions).${budgetNote}`,
      memoryTools + "\n\n",
      rendered,
      startupSection,
      episodeSection,
      globalSection,
      pressureWarning,
    ].join(" ");

    const injectionMode: MemoryInjectionMode = "semantic"; // unified pipeline is always "semantic"
    const metrics = createMemoryInjectionMetrics({
      mode: injectionMode,
      projectFactCount: injectedProjectFactCount,
      edgeCount: 0,
      workingMemoryFactCount: injectedWorkingMemoryFactCount,
      semanticHitCount: injectedSemanticHitCount,
      episodeCount: injectedEpisodeCount,
      globalFactCount: injectedGlobalFactCount,
      payloadChars: injectionContent.length,
      baselineContextTokens: usage?.tokens ?? null,
      userPromptTokensEstimate: estimateTokensFromChars(userText),
    });

    sharedState.memoryTokenEstimate = metrics.estimatedTokens;
    sharedState.lastMemoryInjection = metrics;
    pendingInjectionCalibration = {
      baselineContextTokens: metrics.baselineContextTokens ?? null,
      userPromptTokensEstimate: metrics.userPromptTokensEstimate ?? 0,
    };

    return {
      message: {
        customType: "project-memory",
        content: injectionContent,
        display: false,
      },
    };
  });

  pi.on("agent_end", async (event, _ctx) => {
    if (!pendingInjectionCalibration) return;
    const lastAssistant = [...event.messages].reverse().find((msg: any) =>
      msg.role === "assistant" && msg.stopReason !== "aborted" && msg.stopReason !== "error" && msg.usage,
    ) as any;
    if (!lastAssistant?.usage || !sharedState.lastMemoryInjection) return;

    const observedInputTokens = lastAssistant.usage.input ?? 0;
    const baselineContextTokens = pendingInjectionCalibration.baselineContextTokens;
    const userPromptTokensEstimate = pendingInjectionCalibration.userPromptTokensEstimate;
    const inferredAdditionalPromptTokens = baselineContextTokens !== null
      ? Math.max(0, observedInputTokens - baselineContextTokens - userPromptTokensEstimate)
      : null;
    const estimatedVsObservedDelta = inferredAdditionalPromptTokens !== null
      ? sharedState.lastMemoryInjection.estimatedTokens - inferredAdditionalPromptTokens
      : null;

    sharedState.lastMemoryInjection = {
      ...sharedState.lastMemoryInjection,
      observedInputTokens,
      inferredAdditionalPromptTokens,
      estimatedVsObservedDelta,
    } satisfies MemoryInjectionMetrics;

    pendingInjectionCalibration = null;
  });

  // --- Task-Completion Facts: capture write/edit args before execution ---
  // We listen to tool_call (pre-execution) to grab the file path from input,
  // then use tool_execution_end to confirm success and store the "Recent Work" fact.

  pi.on("tool_call", (event, _ctx) => {
    const name = (event as any).toolName as string | undefined;
    if (name === "write" || name === "edit") {
      const input = (event as any).input as Record<string, unknown> | undefined;
      const filePath = (input?.path ?? input?.file_path) as string | undefined;
      const toolCallId = (event as any).toolCallId as string | undefined;
      if (filePath && toolCallId) {
        pendingWriteEditArgs.set(toolCallId, { toolName: name, path: filePath });
      }
    }
  });

  // --- Background Extraction Triggers ---

  pi.on("tool_execution_end", async (event, ctx) => {
    if (!store) return;

    triggerState.toolCallsSinceExtract++;

    // --- Task-completion facts (Recent Work section) ---
    // Fire-and-forget, non-blocking. Only file writes/edits that succeed.
    if (!event.isError && (event.toolName === "write" || event.toolName === "edit")) {
      const pending = pendingWriteEditArgs.get(event.toolCallId);
      const filePath = pending?.path;
      if (filePath) {
        pendingWriteEditArgs.delete(event.toolCallId);
        const action = event.toolName === "write" ? "Wrote" : "Edited";
        const shortPath = filePath.replace(process.cwd() + "/", "").replace(process.cwd(), "");
        const factContent = `${action} ${shortPath}`;

        // Track for session telemetry
        if (event.toolName === "write") {
          sessionFilesWritten.push(shortPath);
        } else {
          sessionFilesEdited.push(shortPath);
        }

        // Store as "Recent Work" fact with fast decay (halfLife=2d)
        const mind = activeMind();
        try {
          store.storeFact({
            mind,
            section: "Recent Work" as any,
            content: factContent,
            source: "tool-call",
            decayProfile: "recent_work",
          });
        } catch {
          // Best effort — non-blocking
        }
      }
    }

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

  // --- Lifecycle Candidate Ingestion API ---
  
  /**
   * Process a lifecycle candidate through the structured ingestion pipeline.
   * Entry point for design-tree, openspec, and cleave to emit lifecycle candidates.
   * 
   * @param candidate - The lifecycle candidate to process
   * @returns Result indicating whether it was stored, reinforced, or deferred
   */
  function ingestLifecycle(candidate: LifecycleCandidate): LifecycleCandidateResult {
    if (!store) {
      return {
        autoStored: false,
        duplicate: false,
        reason: "Project memory not initialized",
      };
    }
    
    const mind = activeMind();
    return ingestLifecycleCandidate(store, mind, candidate);
  }
  
  /**
   * Process multiple lifecycle candidates in a batch.
   * 
   * @param candidates - Array of lifecycle candidates to process
   * @returns Aggregated batch results
   */
  function ingestLifecycleBatch(candidates: LifecycleCandidate[]): BatchIngestResult {
    if (!store) {
      return {
        autoStored: 0,
        reinforced: 0,
        rejected: 0,
        deferred: 0,
        factIds: [],
      };
    }
    
    const mind = activeMind();
    return ingestLifecycleCandidatesBatch(store, mind, candidates);
  }

  // --- Lifecycle Candidate Queue Ingestion ---

  function drainLifecycleCandidateQueue(ctx: ExtensionContext): void {
    if (!store) return;
    const queue = sharedState.lifecycleCandidateQueue ?? [];
    if (queue.length === 0) return;

    sharedState.lifecycleCandidateQueue = [];

    for (const payload of queue) {
      try {
        if (!Array.isArray(payload.candidates) || payload.candidates.length === 0) continue;

        const mind = activeMind();
        const result = ingestLifecycleCandidatesBatch(store, mind, payload.candidates as LifecycleMemoryCandidate[]);

        for (const factId of result.factIds) {
          addToWorkingMemory(factId);
        }

        if (ctx.hasUI && (result.autoStored > 0 || result.reinforced > 0)) {
          const parts: string[] = [];
          if (result.autoStored > 0) parts.push(`+${result.autoStored} stored`);
          if (result.reinforced > 0) parts.push(`${result.reinforced} reinforced`);
          ctx.ui.notify(`Lifecycle memory (${payload.source}): ${parts.join(", ")}`, "info");
        }
      } catch (error) {
        console.error("[project-memory] Failed to ingest lifecycle candidates:", error);
      }
    }

    updateStatus(ctx);
  }

  function drainFactArchiveQueue(): void {
    if (!store) return;
    const queue = sharedState.factArchiveQueue ?? [];
    if (queue.length === 0) return;

    sharedState.factArchiveQueue = [];

    const mind = activeMind();
    for (const contentPrefix of queue) {
      try {
        // Use prefix lookup (LIKE) instead of FTS5 to avoid syntax errors with special chars like '[', '(', etc.
        const results = store.findFactsByContentPrefix(contentPrefix, mind);
        for (const fact of results) {
          store.archiveFact(fact.id);
        }
      } catch (error) {
        console.error("[project-memory] Failed to archive fact by prefix:", error);
      }
    }
  }

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
    renderCall(_args: any, theme: any) {
      return sciCall("memory_query", "full read", theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      const d = result.details as { facts?: number; mind?: string } | undefined;
      const n = d?.facts ?? 0;
      const mind = d?.mind ?? "project";
      const summary = `${n} facts · ${mind}`;

      if (expanded) {
        const text = result.content?.[0]?.text ?? "";
        // Parse section headers from rendered output
        const lines: string[] = [];
        const sections = text.split(/\n## /).filter(Boolean);
        for (const sec of sections) {
          const heading = sec.split("\n")[0].replace(/^#+\s*/, "").trim();
          const bullets = sec.split("\n").filter((l: string) => l.startsWith("- "));
          if (heading && bullets.length > 0) {
            lines.push(theme.fg("accent", heading) + theme.fg("dim", ` · ${bullets.length}`));
          }
        }
        if (lines.length === 0) lines.push(theme.fg("muted", "empty"));
        return sciExpanded(lines, summary, theme);
      }

      return sciOk(summary, theme);
    },
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
    renderCall(args: any, theme: any) {
      const q = args.query?.length > 50 ? args.query.slice(0, 47) + "…" : args.query;
      const sec = args.section ? ` in:${args.section}` : "";
      return sciCall("memory_recall", `"${q}"${sec}`, theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { method?: string; results?: number; episodes?: number; workingMemorySize?: number } | undefined;
      const n = d?.results ?? 0;
      const method = d?.method ?? "search";
      const epis = d?.episodes ?? 0;
      const summary = n === 0
        ? "no matches"
        : `${n} result${n !== 1 ? "s" : ""} via ${method}` + (epis > 0 ? ` + ${epis} episode${epis !== 1 ? "s" : ""}` : "");

      if (expanded && n > 0) {
        const text = result.content?.[0]?.text ?? "";
        const lines = text.split("\n").filter(Boolean).slice(0, 12).map((line: string) => {
          // Highlight match percentages
          const m = line.match(/\((\d+)% match/);
          if (m) {
            const pct = parseInt(m[1]);
            const color = pct >= 70 ? "success" : pct >= 40 ? "warning" : "muted";
            return line.replace(`${m[1]}% match`, theme.fg(color as any, `${m[1]}%`));
          }
          return line;
        });
        return sciExpanded(lines, summary, theme);
      }

      return n === 0 ? sciErr(summary, theme) : sciOk(summary, theme);
    },
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
    renderCall(args: any, theme: any) {
      const q = args.query?.length > 50 ? args.query.slice(0, 47) + "…" : args.query;
      return sciCall("memory_episodes", `"${q}"`, theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { method?: string; results?: number } | undefined;
      const n = d?.results ?? 0;
      const method = d?.method ?? "recent";
      const summary = n === 0 ? "no episodes" : `${n} episode${n !== 1 ? "s" : ""} (${method})`;

      if (expanded && n > 0) {
        const text = result.content?.[0]?.text ?? "";
        // Extract session titles (bold date: title lines)
        const lines = text.split("\n").filter((l: string) => l.match(/^\d+\.\s+\*\*/)).map((l: string) => {
          const m = l.match(/\*\*(.+?)\*\*/);
          return m ? theme.fg("accent", m[1]) : l;
        }).slice(0, 8);
        return sciExpanded(lines, summary, theme);
      }

      return n === 0 ? sciErr(summary, theme) : sciOk(summary, theme);
    },
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
    renderCall(args: any, theme: any) {
      const n = Array.isArray(args.fact_ids) ? args.fact_ids.length : "?";
      return sciCall("memory_focus", `pin ${n} fact${n !== 1 ? "s" : ""}`, theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      const d = result.details as { workingMemorySize?: number } | undefined;
      const sz = d?.workingMemorySize ?? 0;
      return sciOk(`${sz} facts in working memory`, theme);
    },
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
    renderCall(_args: any, theme: any) {
      return sciCall("memory_release", "clear", theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      const text = result.content?.[0]?.text ?? "cleared";
      return sciOk(text, theme);
    },
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
    renderCall(args: any, theme: any) {
      const sec = args.section ?? "?";
      const preview = args.content?.length > 45 ? args.content.slice(0, 42) + "…" : args.content;
      return sciCall("memory_store", `${sec} · ${preview}`, theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { section?: string; id?: string; reinforced?: boolean; facts?: number; conflicts?: boolean } | undefined;
      const sec = d?.section ?? "?";
      const reinforced = d?.reinforced ?? false;
      const conflicts = d?.conflicts ?? false;
      const icon = reinforced ? "↻" : conflicts ? "⚠" : "✓";
      const verb = reinforced ? "reinforced" : "stored";
      const summary = `${icon} ${verb} → ${sec}` + (d?.facts != null ? ` (${d.facts} total)` : "");

      if (expanded && conflicts) {
        const text = result.content?.[0]?.text ?? "";
        const lines = text.split("\n").filter(Boolean);
        return sciExpanded(lines, summary, theme);
      }

      return sciOk(summary, theme);
    },
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
        trackEmbed(embedFact(result.id).catch(() => {})); // tracked fire-and-forget
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
    renderCall(args: any, theme: any) {
      return sciCall("memory_supersede", `[${args.fact_id}] → ${args.section}`, theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { oldId?: string; newId?: string; section?: string; facts?: number } | undefined;
      return sciOk(`[${d?.oldId}] → [${d?.newId}] in ${d?.section}` + (d?.facts != null ? ` (${d.facts} total)` : ""), theme);
    },
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
    renderCall(args: any, theme: any) {
      const q = args.query?.length > 50 ? args.query.slice(0, 47) + "…" : args.query;
      return sciCall("memory_search_archive", `"${q}"`, theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      const d = result.details as { totalMatches?: number; crossMind?: boolean } | undefined;
      const n = d?.totalMatches ?? 0;
      const cross = d?.crossMind ? " (cross-mind)" : "";
      const summary = n === 0 ? "no archived matches" : `${n} archived fact${n !== 1 ? "s" : ""}${cross}`;

      if (expanded && n > 0) {
        const text = result.content?.[0]?.text ?? "";
        const lines = text.split("\n").filter(Boolean).slice(0, 10);
        return sciExpanded(lines, summary, theme);
      }

      return n === 0 ? sciErr(summary, theme) : sciOk(summary, theme);
    },
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
    renderCall(args: any, theme: any) {
      return sciCall("memory_connect", `${args.source_fact_id} ─${args.relation}→ ${args.target_fact_id}`, theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { reinforced?: boolean; relation?: string } | undefined;
      const verb = d?.reinforced ? "reinforced" : "connected";
      const rel = d?.relation ?? "→";
      return sciOk(`${verb} ─${rel}→`, theme);
    },
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
    renderCall(args: any, theme: any) {
      const n = Array.isArray(args.fact_ids) ? args.fact_ids.length : "?";
      return sciCall("memory_archive", `${n} fact${n !== 1 ? "s" : ""}`, theme);
    },
    renderResult(result: any, { expanded }: any, theme: any) {
      if ((result as any).isError) return sciErr(result.content?.[0]?.text ?? "Error", theme);
      const d = result.details as { archived?: number; remaining?: number } | undefined;
      const archived = d?.archived ?? 0;
      const remaining = d?.remaining;
      const summary = `${archived} archived` + (remaining != null ? ` · ${remaining} remaining` : "");

      if (expanded) {
        const text = result.content?.[0]?.text ?? "";
        const lines = text.split("\n").filter(Boolean).slice(0, 10);
        return sciExpanded(lines, summary, theme);
      }

      return sciOk(summary, theme);
    },
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
    renderCall(_args: any, theme: any) {
      return sciCall("memory_compact", "trigger", theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      const d = result.details as { percent?: string; tokensBefore?: number } | undefined;
      const pct = d?.percent ?? "?";
      const tokens = d?.tokensBefore != null ? `${(d.tokensBefore / 1000).toFixed(0)}k tokens` : "";
      return sciOk(`compacting (was ${pct}${tokens ? `, ${tokens}` : ""})`, theme);
    },
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

  // Expose lifecycle ingestion API to other extensions
  (pi as any).memory = {
    ingestLifecycle,
    ingestLifecycleBatch,
  };

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

  // --- Lifecycle Testing Tool ---

  pi.registerTool({
    name: "memory_ingest_lifecycle",
    label: "Ingest Lifecycle Candidate",
    description: [
      "Internal tool for testing lifecycle candidate ingestion.",
      "Used by design-tree, openspec, and cleave extensions to emit structured lifecycle facts.",
      "Not intended for direct agent use - use memory_store for manual fact creation.",
    ].join(" "),
    promptSnippet: "Test lifecycle candidate ingestion (internal tool)",
    parameters: Type.Object({
      source_kind: Type.Union([
        Type.Literal("design-decision"),
        Type.Literal("design-constraint"),
        Type.Literal("openspec-archive"),
        Type.Literal("openspec-assess"),
        Type.Literal("cleave-outcome"),
        Type.Literal("cleave-bug-fix"),
      ], {
        description: "Source kind that generated this candidate",
      }),
      authority: Type.Union([
        Type.Literal("explicit"),
        Type.Literal("inferred"),
      ], {
        description: "Authority level - explicit auto-stores, inferred needs confirmation",
      }),
      section: StringEnum(
        ["Architecture", "Decisions", "Constraints", "Known Issues", "Patterns & Conventions", "Specs"] as const,
        { description: "Target memory section" },
      ),
      content: Type.String({
        description: "Fact content",
      }),
      artifact_ref_type: Type.Optional(Type.Union([
        Type.Literal("design-node"),
        Type.Literal("openspec-spec"),
        Type.Literal("openspec-baseline"),
        Type.Literal("cleave-review"),
      ], {
        description: "Type of artifact",
      })),
      artifact_ref_path: Type.Optional(Type.String({
        description: "Path or identifier",
      })),
      artifact_ref_sub: Type.Optional(Type.String({
        description: "Optional sub-reference (e.g. decision title, spec section)",
      })),
      supersedes: Type.Optional(Type.String({
        description: "Optional fact ID to supersede",
      })),
    }),
    renderCall(args: any, theme: any) {
      return sciCall("memory_ingest_lifecycle", `${args.source_kind} (${args.authority})`, theme);
    },
    renderResult(result: any, _opts: any, theme: any) {
      const d = result.details as { autoStored?: boolean; duplicate?: boolean; factId?: string; reason?: string } | undefined;
      if (d?.autoStored) {
        return sciOk(d.duplicate ? `↻ reinforced [${d.factId}]` : `✓ stored [${d.factId}]`, theme);
      }
      return sciErr(d?.reason ?? "not stored", theme);
    },
    async execute(_toolCallId: string, params: any, _signal: any, _onUpdate: any, _ctx: any): Promise<any> {
      const candidate: LifecycleCandidate = {
        sourceKind: params.source_kind,
        authority: params.authority,
        section: params.section,
        content: params.content,
        supersedes: params.supersedes,
      };

      if (params.artifact_ref_type && params.artifact_ref_path) {
        candidate.artifactRef = {
          type: params.artifact_ref_type,
          path: params.artifact_ref_path,
          subRef: params.artifact_ref_sub,
        };
      }

      const result = ingestLifecycle(candidate);

      let responseText = "";
      if (result.autoStored) {
        if (result.duplicate) {
          responseText = `✓ Reinforced existing fact [${result.factId}]: ${candidate.content}`;
        } else {
          responseText = `✓ Stored new lifecycle fact [${result.factId}]: ${candidate.content}`;
        }
      } else {
        responseText = `⚠ Candidate not stored: ${result.reason}`;
      }

      return {
        content: [{ type: "text", text: responseText }],
        details: {
          sourceKind: candidate.sourceKind,
          authority: candidate.authority,
          autoStored: result.autoStored,
          duplicate: result.duplicate,
          factId: result.factId,
          reason: result.reason,
        },
      };
    },
  });

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
            ...formatMemoryInjectionMetrics(sharedState.lastMemoryInjection),
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

  pi.registerMessageRenderer("session-exit", (_message, _options, theme) => {
    const data = (_message.details ?? {}) as ExitCardData;
    return sciExitCard(data, theme);
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
          const today = new Date().toISOString().split("T")[0];

          const telemetry: SessionTelemetry = {
            date: today,
            toolCallCount: triggerState.toolCallsSinceExtract,
            filesWritten: [...sessionFilesWritten],
            filesEdited: [...sessionFilesEdited],
          };

          // Fallback chain: Ollama → codex-spark → haiku → template (always succeeds)
          const episodeOutput = await generateEpisodeWithFallback(serialized, telemetry, config, ctx.cwd);

          if (store) {
            const sessionFactIds = [...workingMemory];
            const episodeId = store.storeEpisode({
              mind,
              title: episodeOutput.title,
              narrative: episodeOutput.narrative,
              date: today,
              factIds: sessionFactIds.filter(id => store!.getFact(id)?.status === "active"),
            });

            // Embed episode vector (tracked — shutdown awaits before DB close)
            if (embeddingAvailable) {
              trackEmbed(
                embedText(`${episodeOutput.title} ${episodeOutput.narrative}`)
                  .then(vec => { if (vec && store) store.storeEpisodeVector(episodeId, vec, embeddingModel!); })
                  .catch(() => {}),
              );
            }
          }
          exitEpisodeDone = true;
        } catch {
          // Best effort — don't block exit
        }
      } else {
        exitEpisodeDone = true;
      }

      // Build session-end card data
      const exitData: ExitCardData = {
        factCount: factsAfter,
        factDelta: delta,
        embeddingAvailable,
      };

      // Git state
      try {
        const branchResult = await pi.exec("git", ["branch", "--show-current"], { timeout: 3_000, cwd: ctx.cwd });
        const statusResult = await pi.exec("git", ["status", "--short"], { timeout: 3_000, cwd: ctx.cwd });
        exitData.branch = branchResult.stdout.trim();
        exitData.dirtyCount = statusResult.stdout.trim().split("\n").filter(Boolean).length;
      } catch { /* ignore */ }

      // Design tree
      const dt = sharedState.designTree;
      if (dt && dt.nodeCount > 0) {
        exitData.designNodes = dt.nodeCount;
        exitData.designImplemented = dt.implementedCount;
        exitData.designDecided = dt.decidedCount;
        exitData.designExploring = dt.exploringCount;
      }

      // OpenSpec
      const os = sharedState.openspec;
      if (os && os.changes.length > 0) {
        const active = os.changes.filter(c => c.stage !== "archived");
        if (active.length > 0) {
          exitData.openspecActive = active.map(c => c.name);
        }
      }

      // Embedding coverage
      if (store && embeddingAvailable) {
        const vecCount = store.countFactVectors(mind);
        exitData.embeddingPct = factsAfter > 0 ? Math.round((vecCount / factsAfter) * 100) : 100;
      }

      // Render as a proper sci-ui card in the conversation
      pi.sendMessage({
        customType: "session-exit",
        content: "Session ended.",
        details: exitData,
        display: true,
      });

      // Let the card render before shutdown tears down the TUI
      await new Promise(r => setTimeout(r, 500));

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
