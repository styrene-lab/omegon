/**
 * chronos — Authoritative date and time context from system clock
 *
 * Registers a `chronos` tool that executes the chronos.sh script and returns
 * structured date context. Eliminates AI date calculation errors by providing
 * an authoritative source of truth from the system clock.
 *
 * Also registers a `/chronos` command for interactive use.
 *
 * Subcommands: week (default), month, quarter, relative, iso, epoch, tz, range, all
 */

import { existsSync } from "node:fs";
import { join } from "node:path";
import { StringEnum } from "@mariozechner/pi-ai";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

const CHRONOS_SH = join(import.meta.dirname ?? __dirname, "chronos.sh");

const SUBCOMMANDS = ["week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all"] as const;

export default function chronosExtension(pi: ExtensionAPI) {

	// Ensure the script exists and is executable
	if (!existsSync(CHRONOS_SH)) {
		// Fail silently at load — the tool will report the error at call time
	}

	// ------------------------------------------------------------------
	// chronos tool — callable by the LLM
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "chronos",
		label: "Chronos",
		description:
			"Get authoritative date and time context from the system clock. " +
			"Use before any date calculations, weekly/monthly reporting, relative date references, " +
			"quarter boundaries, or epoch timestamps. Eliminates AI date calculation errors.\n\n" +
			"Subcommands:\n" +
			"  week (default) — Current/previous week boundaries (Mon-Fri)\n" +
			"  month — Current/previous month boundaries\n" +
			"  quarter — Calendar quarter, fiscal year (Oct-Sep)\n" +
			"  relative — Resolve expression like '3 days ago', 'next Monday'\n" +
			"  iso — ISO 8601 week number, year, day-of-year\n" +
			"  epoch — Unix timestamp (seconds and milliseconds)\n" +
			"  tz — Timezone abbreviation and UTC offset\n" +
			"  range — Calendar and business days between two dates\n" +
			"  all — All of the above combined",
		promptSnippet:
			"Authoritative date/time from system clock — use before any date calculations, reporting, or relative date references",
		promptGuidelines: [
			"Call chronos before any date math, weekly/monthly reporting, relative dates, or quarter references — never calculate dates manually",
		],

		parameters: Type.Object({
			subcommand: Type.Optional(
				StringEnum(SUBCOMMANDS, { description: "Subcommand (default: week)" })
			),
			expression: Type.Optional(
				Type.String({ description: "For 'relative': date expression (e.g. '3 days ago', 'next Monday')" })
			),
			from_date: Type.Optional(
				Type.String({ description: "For 'range': start date YYYY-MM-DD" })
			),
			to_date: Type.Optional(
				Type.String({ description: "For 'range': end date YYYY-MM-DD" })
			),
		}),

		async execute(_toolCallId, params, signal, _onUpdate, _ctx) {
			if (!existsSync(CHRONOS_SH)) {
				throw new Error(
					`chronos.sh not found at ${CHRONOS_SH}. ` +
					`Expected alongside the chronos skill.`
				);
			}

			const sub = params.subcommand || "week";
			const args = [CHRONOS_SH, sub];

			if (sub === "relative") {
				if (!params.expression) {
					throw new Error("The 'relative' subcommand requires an 'expression' parameter (e.g. '3 days ago').");
				}
				args.push(params.expression);
			} else if (sub === "range") {
				if (!params.from_date || !params.to_date) {
					throw new Error("The 'range' subcommand requires both 'from_date' and 'to_date' (YYYY-MM-DD).");
				}
				args.push(params.from_date, params.to_date);
			}

			const result = await pi.exec("bash", args, { signal, timeout: 10_000 });

			if (result.code !== 0) {
				throw new Error(`chronos.sh failed (exit ${result.code}):\n${result.stderr || result.stdout}`);
			}

			return {
				content: [{ type: "text", text: result.stdout.trim() }],
				details: { subcommand: sub },
			};
		},
	});

	// ------------------------------------------------------------------
	// /chronos command — interactive shortcut
	// ------------------------------------------------------------------
	pi.registerCommand("chronos", {
		description: "Show date/time context (usage: /chronos [week|month|quarter|iso|epoch|tz|all])",
		getArgumentCompletions: (prefix: string) => {
			const items = SUBCOMMANDS.map((s) => ({ value: s, label: s }));
			const filtered = items.filter((i) => i.value.startsWith(prefix || ""));
			return filtered.length > 0 ? filtered : null;
		},
		handler: async (args, _ctx) => {
			const sub = (args || "").trim() || "week";

			if (!existsSync(CHRONOS_SH)) {
				pi.sendMessage({
					customType: "view",
					content: `❌ chronos.sh not found at \`${CHRONOS_SH}\``,
					display: true,
				});
				return;
			}

			const cliArgs = [CHRONOS_SH, sub];
			const result = await pi.exec("bash", cliArgs, { timeout: 10_000 });

			if (result.code !== 0) {
				pi.sendMessage({
					customType: "view",
					content: `❌ chronos.sh failed:\n\`\`\`\n${result.stderr || result.stdout}\n\`\`\``,
					display: true,
				});
				return;
			}

			pi.sendMessage({
				customType: "view",
				content: `**Chronos**\n\n\`\`\`\n${result.stdout.trim()}\n\`\`\``,
				display: true,
			});
		},
	});
}
