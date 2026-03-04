/**
 * session-log — Append-only session tracking
 *
 * Registers `/session-log` command that appends a structured entry to
 * the .session_log file in the repository root (or reads existing entries).
 *
 * Subcommands:
 *   /session-log           — Generate and append a new entry for this session
 *   /session-log read      — Show recent entries
 *   /session-log read <n>  — Show last n entries
 */

import { existsSync, readFileSync } from "node:fs";
import { join, basename } from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const SESSION_LOG_HEADER = `# Session Log

Append-only record of development sessions. Read recent entries for context.
`;

export default function sessionLogExtension(pi: ExtensionAPI) {

	// ------------------------------------------------------------------
	// Auto-read session log on session start for context
	// ------------------------------------------------------------------
	pi.on("session_start", async (_event, ctx) => {
		const logPath = join(ctx.cwd, ".session_log");
		if (existsSync(logPath)) {
			try {
				const content = readFileSync(logPath, "utf-8");
				const lines = content.split("\n");
				// Show last ~80 lines as context (lightweight, non-intrusive)
				const tail = lines.slice(-80).join("\n").trim();
				if (tail) {
					// Inject as a context message so the LLM knows recent history
					pi.sendMessage({
						customType: "session-log-context",
						content: `Recent .session_log entries (last 80 lines):\n\n${tail}`,
						display: false,  // Don't clutter the user's display
					}, { deliverAs: "nextTurn" });
				}
			} catch { /* ignore read errors */ }
		}
	});

	// ------------------------------------------------------------------
	// /session-log command
	// ------------------------------------------------------------------
	pi.registerCommand("session-log", {
		description: "Append or read .session_log entries (usage: /session-log [read [n]])",
		getArgumentCompletions: (prefix: string) => {
			const items = [
				{ value: "read", label: "read", description: "Show recent entries" },
			];
			const filtered = items.filter((i) => i.value.startsWith(prefix || ""));
			return filtered.length > 0 ? filtered : null;
		},
		handler: async (args, ctx) => {
			const trimmed = (args || "").trim();
			const cwd = ctx.cwd;
			const logPath = join(cwd, ".session_log");

			// ----------------------------------------------------------
			// /session-log read [n]
			// ----------------------------------------------------------
			if (trimmed.startsWith("read")) {
				if (!existsSync(logPath)) {
					pi.sendMessage({
						customType: "view",
						content: `No .session_log found at \`${logPath}\``,
						display: true,
					});
					return;
				}

				const content = readFileSync(logPath, "utf-8");
				const nArg = trimmed.replace(/^read\s*/, "").trim();
				const n = nArg ? parseInt(nArg, 10) : 5;

				// Split into entries by ## headings
				const entries = content.split(/^(?=## \d{4}-\d{2}-\d{2})/m);
				const header = entries[0]?.startsWith("#") && !entries[0]?.startsWith("## 2") ? entries.shift() : "";
				const recent = entries.slice(-n);

				if (recent.length === 0) {
					pi.sendMessage({
						customType: "view",
						content: "No entries found in `.session_log`",
						display: true,
					});
					return;
				}

				const display = recent.join("\n").trim();
				pi.sendMessage({
					customType: "view",
					content: `**Recent .session_log entries** (${recent.length} of ${entries.length}):\n\n${display}`,
					display: true,
				});
				return;
			}

			// ----------------------------------------------------------
			// /session-log  (generate + append)
			// ----------------------------------------------------------

			// Get today's date
			let today = new Date().toISOString().slice(0, 10);
			try {
				const dateResult = await pi.exec("date", ["+%Y-%m-%d"], { timeout: 3_000 });
				today = dateResult.stdout.trim() || today;
			} catch { /* use JS date */ }

			// Get git info for context
			let branch = "";
			let recentCommits = "";
			try {
				const branchResult = await pi.exec("git", ["branch", "--show-current"], { timeout: 5_000, cwd });
				branch = branchResult.stdout.trim();
			} catch { /* ignore */ }
			try {
				const logResult = await pi.exec(
					"git", ["log", "--oneline", "-5", "--no-decorate"],
					{ timeout: 5_000, cwd },
				);
				recentCommits = logResult.stdout.trim();
			} catch { /* ignore */ }

			// Bootstrap .session_log if it doesn't exist
			const logExists = existsSync(logPath);

			// Build the prompt for the LLM to generate and append the entry
			const prompt = [
				`Analyze the conversation context and append a session log entry to \`${logPath}\`.`,
				"",
				logExists
					? "The file already exists — append to the end."
					: `The file does not exist yet. Create it with this header first:\n\n\`\`\`\n${SESSION_LOG_HEADER}\`\`\`\n\nThen append the entry.`,
				"",
				`**Today's date:** ${today}`,
				branch ? `**Branch:** ${branch}` : "",
				recentCommits ? `**Recent commits:**\n\`\`\`\n${recentCommits}\n\`\`\`` : "",
				"",
				"**Entry format** (use exactly this structure):",
				"",
				`\`\`\`markdown`,
				`## ${today} — Brief Topic Title`,
				"",
				"### Context",
				"One-line description of what prompted this session.",
				"",
				"### Completed",
				"- Concise bullet points of what was accomplished",
				"- Focus on outcomes, not process",
				"",
				"### Decisions",
				"- Choices made and brief rationale",
				"",
				"### Open Threads",
				"- Unfinished work for future sessions",
				"- Known issues discovered",
				"```",
				"",
				"**Guidelines:**",
				"- Keep entries concise — they should trigger memory, not replace documentation",
				"- Focus on outcomes, decisions, and open threads",
				"- Do NOT overwrite or edit previous entries",
			].filter(Boolean).join("\n");

			pi.sendUserMessage(prompt, { deliverAs: "followUp" });
		},
	});
}
