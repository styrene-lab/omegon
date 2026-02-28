/**
 * Project Memory — Extraction v2
 *
 * Updated extraction for SQLite-backed fact store.
 * The extraction agent outputs JSONL actions instead of rewriting a markdown file.
 *
 * Action types:
 *   observe   — "I see this fact in the conversation" (reinforces or adds)
 *   reinforce — "This existing fact is still true" (by ID)
 *   supersede — "This new fact replaces that old one" (by ID + new content)
 *   archive   — "This fact appears stale/wrong" (by ID)
 */

import { spawn, type ChildProcess } from "node:child_process";
import type { MemoryConfig } from "./types.js";
import type { Fact } from "./factstore.js";

/**
 * Build the extraction prompt for JSONL output.
 * Includes current facts with IDs so the agent can reference them.
 */
function buildExtractionPrompt(maxLines: number): string {
  return `You are a project memory curator. You receive:
1. Current active facts (with IDs) from the project's memory database
2. Recent conversation context from a coding session

Your job: output JSONL (one JSON object per line) describing what you observed.

ACTION TYPES:

{"type":"observe","section":"Architecture","content":"The project uses SQLite for storage"}
  → You saw evidence of this fact in the conversation. If it already exists, it gets reinforced.
    If it's new, it gets added.

{"type":"reinforce","id":"abc123"}
  → An existing fact (by ID) is confirmed still true by the conversation context.

{"type":"supersede","id":"abc123","section":"Architecture","content":"The project migrated from SQLite to PostgreSQL"}
  → A specific existing fact is wrong/outdated. Provide the replacement.

{"type":"archive","id":"abc123"}
  → A specific existing fact is clearly wrong, obsolete, or no longer relevant.

RULES:
- Output ONLY valid JSONL. One JSON object per line. No commentary, no explanation.
- Focus on DURABLE technical facts — architecture, decisions, constraints, patterns, bugs.
- DO NOT output facts about transient details (debugging steps, file contents, command output).
- DO NOT output facts that are obvious from reading code (basic imports, boilerplate).
- Prefer "observe" for new facts. Use "supersede" only when you can identify the specific old fact being replaced.
- Use "reinforce" when the conversation confirms an existing fact without changing it.
- Use "archive" sparingly — only when a fact is clearly contradicted.
- Keep fact content self-contained and concise (one line, no bullet prefix).
- Valid sections: Architecture, Decisions, Constraints, Known Issues, Patterns & Conventions

TARGET: aim for at most ${maxLines} active facts total. If the memory is near capacity, use "archive" on the least relevant facts to make room.

If the conversation contains nothing worth remembering, output nothing.`;
}

/**
 * Format current facts for the extraction agent's input.
 * Shows facts with IDs so the agent can reference them.
 */
export function formatFactsForExtraction(facts: Fact[]): string {
  if (facts.length === 0) return "(no existing facts)";

  const lines: string[] = [];
  let currentSection = "";

  for (const fact of facts) {
    if (fact.section !== currentSection) {
      currentSection = fact.section;
      lines.push(`\n## ${currentSection}`);
    }
    const date = fact.created_at.split("T")[0];
    const rc = fact.reinforcement_count;
    lines.push(`[${fact.id}] ${fact.content} (${date}, reinforced ${rc}x)`);
  }

  return lines.join("\n");
}

/** Currently running extraction process */
let activeExtractionProc: ChildProcess | null = null;

/**
 * Run extraction against conversation context.
 * Returns raw JSONL output from the extraction agent.
 */
export async function runExtractionV2(
  cwd: string,
  currentFacts: Fact[],
  recentConversation: string,
  config: MemoryConfig,
): Promise<string> {
  const prompt = buildExtractionPrompt(config.maxLines);
  const factsFormatted = formatFactsForExtraction(currentFacts);

  const userMessage = [
    "Current active facts:\n",
    factsFormatted,
    "\n\n---\n\nRecent conversation:\n\n",
    recentConversation,
    "\n\nOutput JSONL actions based on what you observe.",
  ].join("");

  return new Promise<string>((resolve, reject) => {
    if (activeExtractionProc) {
      reject(new Error("Extraction already in progress"));
      return;
    }

    const args = [
      "--model",
      config.extractionModel,
      "--no-session",
      "--no-tools",
      "--no-extensions",
      "--no-skills",
      "--no-themes",
      "--thinking",
      "off",
      "--system-prompt",
      prompt,
      "-p",
      userMessage,
    ];

    const proc = spawn("pi", args, {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });
    activeExtractionProc = proc;

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (d: Buffer) => {
      stdout += d.toString();
    });
    proc.stderr.on("data", (d: Buffer) => {
      stderr += d.toString();
    });

    let escalationTimer: ReturnType<typeof setTimeout> | null = null;
    const timeout = setTimeout(() => {
      proc.kill("SIGTERM");
      escalationTimer = setTimeout(() => {
        if (!proc.killed) proc.kill("SIGKILL");
      }, 5000);
      reject(new Error("Extraction timed out"));
    }, config.extractionTimeout);

    proc.on("close", (code) => {
      clearTimeout(timeout);
      if (escalationTimer) clearTimeout(escalationTimer);
      activeExtractionProc = null;
      const output = stdout.trim();
      if (code === 0 && output) {
        // Strip code fences if the model wraps output despite instructions
        const cleaned = output
          .replace(/^```(?:jsonl?|json)?\n?/, "")
          .replace(/\n?```\s*$/, "");
        resolve(cleaned);
      } else if (code === 0 && !output) {
        // No output = nothing to remember
        resolve("");
      } else {
        reject(new Error(`Extraction failed (exit ${code}): ${stderr.slice(0, 500)}`));
      }
    });

    proc.on("error", (err) => {
      clearTimeout(timeout);
      activeExtractionProc = null;
      reject(err);
    });
  });
}
