/**
 * chronos — Authoritative date and time context from system clock
 *
 * Pure TypeScript implementation — no shell dependencies.
 * Registers a `chronos` tool and `/chronos` command.
 *
 * Subcommands: week (default), month, quarter, relative, iso, epoch, tz, range, all
 */

import { StringEnum } from "../lib/typebox-helpers";
import type { ExtensionAPI } from "@styrene-lab/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import {
	computeWeek,
	computeMonth,
	computeQuarter,
	computeRelative,
	computeIso,
	computeEpoch,
	computeTz,
	computeRange,
	computeAll,
} from "./chronos";

const SUBCOMMANDS = ["week", "month", "quarter", "relative", "iso", "epoch", "tz", "range", "all"] as const;

function executeChronos(params: { subcommand?: string; expression?: string; from_date?: string; to_date?: string }): string {
	const sub = params.subcommand || "week";

	switch (sub) {
		case "week": return computeWeek();
		case "month": return computeMonth();
		case "quarter": return computeQuarter();
		case "relative":
			if (!params.expression) {
				throw new Error("The 'relative' subcommand requires an 'expression' parameter (e.g. '3 days ago').");
			}
			return computeRelative(params.expression);
		case "iso": return computeIso();
		case "epoch": return computeEpoch();
		case "tz": return computeTz();
		case "range":
			if (!params.from_date || !params.to_date) {
				throw new Error("The 'range' subcommand requires both 'from_date' and 'to_date' (YYYY-MM-DD).");
			}
			return computeRange(params.from_date, params.to_date);
		case "all": return computeAll();
		default: throw new Error(`Unknown subcommand: ${sub}`);
	}
}

export default function chronosExtension(pi: ExtensionAPI) {

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

		async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
			const result = executeChronos(params);
			return {
				content: [{ type: "text", text: result }],
				details: { subcommand: params.subcommand || "week" },
			};
		},
	});

	pi.registerCommand("chronos", {
		description: "Show date/time context (usage: /chronos [week|month|quarter|iso|epoch|tz|all])",
		getArgumentCompletions: (prefix: string) => {
			const items = SUBCOMMANDS.map((s) => ({ value: s, label: s }));
			const filtered = items.filter((i) => i.value.startsWith(prefix || ""));
			return filtered.length > 0 ? filtered : null;
		},
		handler: async (args, _ctx) => {
			const sub = (args || "").trim() || "week";
			try {
				const result = executeChronos({ subcommand: sub });
				pi.sendMessage({
					customType: "view",
					content: `**Chronos**\n\n\`\`\`\n${result}\n\`\`\``,
					display: true,
				});
			} catch (err: unknown) {
				const msg = err instanceof Error ? err.message : String(err);
				pi.sendMessage({
					customType: "view",
					content: `❌ ${msg}`,
					display: true,
				});
			}
		},
	});
}
