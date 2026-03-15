/**
 * Sci-UI — shared visual primitives for Alpharius-styled tool call rendering.
 *
 * Design language:
 *   Call line:   ◈──{ tool_name }── summary text ──────────────────────────
 *   Loading:     ▶░░░░░▓▒{ tool_name }░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
 *   Result:        ╰── ✓ compact summary
 *   Expanded:      │ line 1
 *                  │ line 2
 *                  ╰── N lines
 *   Banner:      ── ◈ label ──────────────────────────────────────────────
 *                  content line
 *
 * NOTE: All classes use explicit field declarations (not constructor parameter
 * properties) to remain compatible with Node.js strip-only TypeScript mode.
 */
import { truncateToWidth, visibleWidth } from "@cwilson613/pi-tui";
import type { Theme } from "@cwilson613/pi-coding-agent";

export interface SciComponent {
	render(width: number): string[];
	invalidate(): void;
}

// ─── Tool glyphs by name ───────────────────────────────────────────────────

export const TOOL_GLYPHS: Record<string, string> = {
	// Core tools
	read: "▸",
	edit: "▸",
	write: "▸",
	bash: "▸",
	grep: "▸",
	find: "▸",
	ls: "▸",
	// Extension tools
	design_tree: "◈",
	design_tree_update: "◈",
	openspec_manage: "◎",
	memory_store: "⌗",
	memory_recall: "⌗",
	memory_query: "⌗",
	memory_focus: "⌗",
	memory_release: "⌗",
	memory_supersede: "⌗",
	memory_archive: "⌗",
	memory_compact: "⌗",
	memory_connect: "⌗",
	memory_search_archive: "⌗",
	memory_episodes: "⌗",
	memory_ingest_lifecycle: "⌗",
	cleave_run: "⚡",
	cleave_assess: "⚡",
	whoami: "⊙",
	chronos: "◷",
	web_search: "⌖",
	render_diagram: "⬡",
	render_native_diagram: "⬡",
	render_excalidraw: "⬡",
	render_composition_still: "⬡",
	render_composition_video: "⬡",
	generate_image_local: "⬡",
	view: "⬡",
	// Inference / model tools
	set_model_tier: "◆",
	set_thinking_level: "◆",
	ask_local_model: "◆",
	list_local_models: "◆",
	manage_ollama: "◆",
	switch_to_offline_driver: "◆",
	// Profile / misc
	manage_tools: "⊞",
};

export function glyphFor(toolName: string): string {
	return TOOL_GLYPHS[toolName] ?? "▸";
}

// ─── SciCallLine ──────────────────────────────────────────────────────────
//
//   ◈──{ design_tree }── action:node_id ─────────────────────────────────

export class SciCallLine implements SciComponent {
	glyph: string;
	toolName: string;
	summary: string;
	theme: Theme;

	constructor(glyph: string, toolName: string, summary: string, theme: Theme) {
		this.glyph = glyph;
		this.toolName = toolName;
		this.summary = summary;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const g = th.fg("accent", this.glyph);
		const dashes = th.fg("dim", "──");
		const openBracket = th.fg("border", "{");
		const closeBracket = th.fg("border", "}");
		const name = th.fg("accent", this.toolName);
		const sep = th.fg("dim", "──");
		const sumText = this.summary
			? " " + th.fg("muted", this.summary) + " "
			: " ";

		const core = `${g}${dashes}${openBracket}${name}${closeBracket}${sep}${sumText}`;
		const coreVw = visibleWidth(core);
		const fillLen = Math.max(0, width - coreVw);
		const fill = th.fg("dim", "─".repeat(fillLen));

		return [truncateToWidth(core + fill, width)];
	}

	invalidate(): void {}
}

// ─── SciLoadingLine ───────────────────────────────────────────────────────
//
//   ▶░░░░░▓▒{ tool_name }░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
//
// A bright block scans left→right while a tool is pending.

export class SciLoadingLine implements SciComponent {
	toolName: string;
	theme: Theme;

	constructor(toolName: string, theme: Theme) {
		this.toolName = toolName;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const label = `{ ${this.toolName} }`;
		const labelVw = visibleWidth(label);
		const barWidth = Math.max(4, width - labelVw - 2);
		const frame = Math.floor(Date.now() / 120) % barWidth;

		const bar = Array.from({ length: barWidth }, (_, i) => {
			if (i === frame) return th.fg("accent", "▓");
			if (i === (frame + 1) % barWidth) return th.fg("muted", "▒");
			return th.fg("dim", "░");
		}).join("");

		const line =
			th.fg("accent", "▶") +
			bar +
			th.fg("muted", "{") +
			th.fg("accent", ` ${this.toolName} `) +
			th.fg("muted", "}");

		return [truncateToWidth(line, width)];
	}

	invalidate(): void {}
}

// ─── SciResult (compact / collapsed) ─────────────────────────────────────
//
//   ╰── ✓ compact summary
//   ╰── ✕ error text
//   ╰── · pending

export class SciResult implements SciComponent {
	summary: string;
	status: "success" | "error" | "pending";
	theme: Theme;

	constructor(summary: string, status: "success" | "error" | "pending", theme: Theme) {
		this.summary = summary;
		this.status = status;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const cap = th.fg("dim", "  ╰──");
		const dot =
			this.status === "success"
				? th.fg("success", " ✓")
				: this.status === "error"
					? th.fg("error", " ✕")
					: th.fg("dim", " ·");
		const capVw = visibleWidth(cap + dot);
		const textLen = Math.max(1, width - capVw - 1);
		const text = " " + th.fg("muted", truncateToWidth(this.summary, textLen));
		return [truncateToWidth(cap + dot + text, width)];
	}

	invalidate(): void {}
}

// ─── SciExpandedResult ───────────────────────────────────────────────────
//
//   │ line 1
//   │ line 2
//   ╰── footer summary

export class SciExpandedResult implements SciComponent {
	lines: string[];
	footerSummary: string;
	theme: Theme;

	constructor(lines: string[], footerSummary: string, theme: Theme) {
		this.lines = lines;
		this.footerSummary = footerSummary;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const innerWidth = Math.max(1, width - 4);
		const result: string[] = [];
		for (const line of this.lines) {
			result.push(th.fg("dim", "  │") + " " + truncateToWidth(line, innerWidth));
		}
		result.push(
			th.fg("dim", "  ╰──") +
			" " +
			th.fg("muted", truncateToWidth(this.footerSummary, Math.max(1, width - 8))),
		);
		return result;
	}

	invalidate(): void {}
}

// ─── SciBanner (custom message renderer) ─────────────────────────────────
//
//   ── ◈ label ──────────────────────────────────────────────────────────
//     content line 1

export class SciBanner implements SciComponent {
	glyph: string;
	label: string;
	contentLines: string[];
	theme: Theme;

	constructor(glyph: string, label: string, contentLines: string[], theme: Theme) {
		this.glyph = glyph;
		this.label = label;
		this.contentLines = contentLines;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const midText = ` ${th.fg("accent", this.glyph)} ${th.fg("muted", this.label)} `;
		const midVw = visibleWidth(midText);
		const leftLen = 2;
		const rightLen = Math.max(0, width - midVw - leftLen);
		const header =
			th.fg("dim", "──") +
			midText +
			th.fg("dim", "─".repeat(rightLen));

		const result = [truncateToWidth(header, width)];
		for (const line of this.contentLines) {
			result.push(truncateToWidth("  " + line, width));
		}
		return result;
	}

	invalidate(): void {}
}

// ─── Convenience builders ─────────────────────────────────────────────────

/** Build a SciCallLine from a tool name + summary string. */
export function sciCall(toolName: string, summary: string, theme: Theme): SciCallLine {
	return new SciCallLine(glyphFor(toolName), toolName, summary, theme);
}

/** Build a SciLoadingLine for use during isPartial. */
export function sciLoading(toolName: string, theme: Theme): SciLoadingLine {
	return new SciLoadingLine(toolName, theme);
}

/** Compact success result line. */
export function sciOk(summary: string, theme: Theme): SciResult {
	return new SciResult(summary, "success", theme);
}

/** Compact error result line. */
export function sciErr(summary: string, theme: Theme): SciResult {
	return new SciResult(summary, "error", theme);
}

/** Compact pending result line. */
export function sciPending(summary: string, theme: Theme): SciResult {
	return new SciResult(summary, "pending", theme);
}

/** Expanded result with bordered body. */
export function sciExpanded(lines: string[], footer: string, theme: Theme): SciExpandedResult {
	return new SciExpandedResult(lines, footer, theme);
}

/** Banner for message renderers. */
export function sciBanner(glyph: string, label: string, lines: string[], theme: Theme): SciBanner {
	return new SciBanner(glyph, label, lines, theme);
}

// ─── SciExitCard ─────────────────────────────────────────────────────────
//
//   ── ⏛ session:end ──────────────────────────────────────────────────
//   │  main · clean              ◈ 80 nodes · 69 implemented
//   │  📋 2 active changes       🧠 1462 facts (+3) · 94% indexed
//   ╰──────────────────────────────────────────────────────────────────

export interface ExitCardData {
	branch?: string;
	dirtyCount?: number;
	designNodes?: number;
	designImplemented?: number;
	designDecided?: number;
	designExploring?: number;
	openspecActive?: string[];
	factCount: number;
	factDelta: number;
	embeddingPct?: number;
	embeddingAvailable: boolean;
}

export class SciExitCard implements SciComponent {
	data: ExitCardData;
	theme: Theme;

	constructor(data: ExitCardData, theme: Theme) {
		this.data = data;
		this.theme = theme;
	}

	render(width: number): string[] {
		const th = this.theme;
		const d = this.data;
		const innerW = Math.max(1, width - 4);

		// Header
		const label = ` ${th.fg("accent", "⏛")} ${th.fg("muted", "session:end")} `;
		const labelVw = visibleWidth(label);
		const headerFill = Math.max(0, width - labelVw - 2);
		const header = th.fg("dim", "──") + label + th.fg("dim", "─".repeat(headerFill));
		const pipe = th.fg("dim", "  │");

		const lines: string[] = [truncateToWidth(header, width)];

		// Row 1: git + design tree (two columns)
		const gitPart = d.branch
			? th.fg("success", d.branch) +
				th.fg("dim", " · ") +
				(d.dirtyCount && d.dirtyCount > 0
					? th.fg("warning", `${d.dirtyCount} dirty`)
					: th.fg("muted", "clean"))
			: null;

		const dtPart = d.designNodes && d.designNodes > 0
			? th.fg("accent", "◈") + " " +
				th.fg("muted", `${d.designNodes} nodes`) +
				th.fg("dim", " · ") +
				th.fg("success", `${d.designImplemented ?? 0}✓`) +
				(d.designDecided ? th.fg("dim", " · ") + th.fg("muted", `${d.designDecided}●`) : "") +
				(d.designExploring ? th.fg("dim", " · ") + th.fg("accent", `${d.designExploring}◐`) : "")
			: null;

		if (gitPart && dtPart) {
			const gitVw = visibleWidth(gitPart);
			const dtVw = visibleWidth(dtPart);
			const gap = Math.max(2, innerW - gitVw - dtVw);
			lines.push(truncateToWidth(pipe + " " + gitPart + " ".repeat(gap) + dtPart, width));
		} else if (gitPart) {
			lines.push(truncateToWidth(pipe + " " + gitPart, width));
		} else if (dtPart) {
			lines.push(truncateToWidth(pipe + " " + dtPart, width));
		}

		// Row 2: openspec + memory (two columns)
		const osPart = d.openspecActive && d.openspecActive.length > 0
			? th.fg("muted", `${d.openspecActive.length} active`) +
				th.fg("dim", " ─ ") +
				th.fg("muted", d.openspecActive.slice(0, 3).join(", ")) +
				(d.openspecActive.length > 3 ? th.fg("dim", `…+${d.openspecActive.length - 3}`) : "")
			: null;

		const deltaStr = d.factDelta > 0
			? th.fg("success", ` +${d.factDelta}`)
			: d.factDelta < 0
				? th.fg("warning", ` ${d.factDelta}`)
				: "";
		const idxStr = d.embeddingAvailable && d.embeddingPct !== undefined
			? th.fg("dim", " · ") + th.fg("muted", `${d.embeddingPct}% indexed`)
			: !d.embeddingAvailable
				? th.fg("dim", " · ") + th.fg("muted", "semantic off")
				: "";
		const memPart = th.fg("accent", "⌗") + " " +
			th.fg("muted", `${d.factCount} facts`) + deltaStr + idxStr;

		if (osPart) {
			const osVw = visibleWidth(osPart);
			const memVw = visibleWidth(memPart);
			const gap = Math.max(2, innerW - osVw - memVw);
			lines.push(truncateToWidth(pipe + " " + osPart + " ".repeat(gap) + memPart, width));
		} else {
			lines.push(truncateToWidth(pipe + " " + memPart, width));
		}

		// Footer rule
		const footerFill = Math.max(0, width - 5);
		lines.push(th.fg("dim", "  ╰" + "─".repeat(footerFill)));

		return lines;
	}

	invalidate(): void {}
}

export function sciExitCard(data: ExitCardData, theme: Theme): SciExitCard {
	return new SciExitCard(data, theme);
}
