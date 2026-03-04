/**
 * distill — Context distillation for session handoff
 *
 * Registers `/distill` command that analyzes the current conversation context
 * and produces a portable distillation summary for bootstrapping a fresh session.
 *
 * The extension handles the mechanical parts (directory creation, file writing,
 * git status collection) and delegates the summarization to the LLM via
 * sendUserMessage.
 */

import { existsSync, mkdirSync } from "node:fs";
import { join, basename } from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

export default function distillExtension(pi: ExtensionAPI) {

	pi.registerCommand("distill", {
		description: "Create a portable session distillation for fresh context bootstrap",
		handler: async (_args, ctx) => {
			// Gather mechanical context the LLM will need
			const cwd = ctx.cwd;
			const repoName = basename(cwd);

			// Git status
			let branch = "unknown";
			let recentCommits = "";
			let uncommitted = "";
			try {
				const branchResult = await pi.exec("git", ["branch", "--show-current"], { timeout: 5_000, cwd });
				branch = branchResult.stdout.trim() || "HEAD detached";
			} catch { /* ignore */ }
			try {
				const logResult = await pi.exec(
					"git", ["log", "--oneline", "-10", "--no-decorate"],
					{ timeout: 5_000, cwd },
				);
				recentCommits = logResult.stdout.trim();
			} catch { /* ignore */ }
			try {
				const statusResult = await pi.exec("git", ["status", "--porcelain"], { timeout: 5_000, cwd });
				uncommitted = statusResult.stdout.trim() || "(clean)";
			} catch { /* ignore */ }

			// Ensure distillation directory exists
			const distillDir = join(cwd, ".pi", "distillations");
			mkdirSync(distillDir, { recursive: true });

			// Generate a timestamp-based filename
			const now = new Date();
			const ts = now.toISOString().replace(/[:.]/g, "-").slice(0, 19);

			// Build the prompt for the LLM to do the actual summarization
			const prompt = [
				"Analyze the full conversation context and create a session distillation.",
				"",
				"**Repository context (already gathered):**",
				`- Working directory: ${cwd}`,
				`- Repository: ${repoName}`,
				`- Branch: ${branch}`,
				`- Recent commits:`,
				"```",
				recentCommits || "(no commits)",
				"```",
				`- Uncommitted changes:`,
				"```",
				uncommitted,
				"```",
				"",
				"**Instructions:**",
				"",
				`Write a distillation file to \`${distillDir}/${ts}-<slug>.md\` where <slug> is a 2-3 word kebab-case description of the session topic.`,
				"",
				"Use this structure:",
				"",
				"```markdown",
				"# Session Distillation: <brief-title>",
				"",
				"Generated: <timestamp>",
				`Working Directory: ${cwd}`,
				`Repository: ${repoName}`,
				"",
				"## Session Overview",
				"<2-3 sentence summary of what was accomplished>",
				"",
				"## Technical State",
				"### Repository Status",
				`- Branch: ${branch}`,
				"- Recent commits: <summarize>",
				"- Uncommitted changes: <summarize>",
				"",
				"### Key Changes This Session",
				"<bulleted list of significant modifications>",
				"",
				"## Decisions Made",
				"<numbered list with brief rationale>",
				"",
				"## Pending Items",
				"### Incomplete Work",
				"### Known Issues",
				"### Planned Next Steps",
				"",
				"## Critical Context",
				"<information that would be difficult to reconstruct>",
				"",
				"## File Reference",
				"Key files for continuation:",
				"- <path>: <purpose>",
				"```",
				"",
				"After writing the file, output a handoff block like:",
				"",
				"```",
				"Distillation saved to: <path>",
				"",
				"To continue in a fresh session, copy this prompt:",
				"---",
				"Continue from distillation: <path>",
				"Read the distillation file and confirm you understand the context before proceeding.",
				"---",
				"```",
			].join("\n");

			pi.sendUserMessage(prompt, { deliverAs: "followUp" });
		},
	});
}
