/**
 * style — /style command for the Verdant design system
 *
 * Registers `/style [subcommand]` as an interactive command.
 * Subcommands: (none), palette, d2, excalidraw, check <file>
 */

import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

// ---------------------------------------------------------------------------
// Palette data — single source of truth
// ---------------------------------------------------------------------------

const CORE_PALETTE = {
	primary:       "#3dc9b0",
	primaryMuted:  "#4a9e90",
	primaryBright: "#8ac4b8",
	fg:            "#d4e8e4",
	mutedFg:       "#8a9a96",
	dimFg:         "#5c6b67",
	bg:            "#0c0e12",
	cardBg:        "#151c20",
	surfaceBg:     "#1a2428",
	borderColor:   "#2d4a47",
	borderDim:     "#1e3533",
};

const SIGNALS = {
	green:  "#34d399",
	red:    "#f44747",
	orange: "#e98100",
	yellow: "#e9c400",
};

const EXCALIDRAW_SEMANTICS: Record<string, { fill: string; stroke: string; use: string }> = {
	primary:   { fill: "#3b82f6", stroke: "#1e3a5f", use: "Default components, neutral nodes" },
	secondary: { fill: "#60a5fa", stroke: "#1e3a5f", use: "Supporting/related components" },
	tertiary:  { fill: "#93c5fd", stroke: "#1e3a5f", use: "Third-level, background detail" },
	start:     { fill: "#fed7aa", stroke: "#c2410c", use: "Entry points, triggers, inputs" },
	end:       { fill: "#a7f3d0", stroke: "#047857", use: "Outputs, completion, results" },
	decision:  { fill: "#fef3c7", stroke: "#b45309", use: "Conditionals, branches, choices" },
	ai:        { fill: "#ddd6fe", stroke: "#6d28d9", use: "AI/LLM components, inference" },
	warning:   { fill: "#fee2e2", stroke: "#dc2626", use: "Warnings, degraded states" },
	error:     { fill: "#fecaca", stroke: "#b91c1c", use: "Error states, failures" },
	evidence:  { fill: "#1e293b", stroke: "#334155", use: "Code snippets, data samples" },
	inactive:  { fill: "#dbeafe", stroke: "#1e40af", use: "Disabled, inactive, future-state" },
};

const D2_STYLE_TEMPLATE = `# Verdant D2 style template — paste into your .d2 file

# Primary component
component: Label {
  style: {
    fill: "#3b82f6"
    stroke: "#1e3a5f"
    font-color: "#ffffff"
    border-radius: 8
  }
}

# Start / entry point
entry: Trigger {
  style: {
    fill: "#fed7aa"
    stroke: "#c2410c"
    font-color: "#374151"
  }
}

# End / output
result: Output {
  style: {
    fill: "#a7f3d0"
    stroke: "#047857"
    font-color: "#374151"
  }
}

# Connection
entry -> component -> result {
  style: {
    stroke: "#3dc9b0"
    font-color: "#d4e8e4"
  }
}

# Container
group: Infrastructure {
  style: {
    fill: "#0c0e12"
    stroke: "#2d4a47"
    font-color: "#8ac4b8"
  }
}`;

// Collect all known hex values for color auditing
const ALL_TOKENS: Record<string, string> = {
	...CORE_PALETTE,
	...SIGNALS,
};
for (const [name, colors] of Object.entries(EXCALIDRAW_SEMANTICS)) {
	ALL_TOKENS[`excalidraw.${name}.fill`] = colors.fill;
	ALL_TOKENS[`excalidraw.${name}.stroke`] = colors.stroke;
}

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

function quickRef(): string {
	return [
		"**Verdant Design System — Quick Reference**",
		"",
		"```",
		"BACKGROUNDS          ACCENTS              SIGNALS",
		`bg:       ${CORE_PALETTE.bg}    primary:    ${CORE_PALETTE.primary}  green:  ${SIGNALS.green}`,
		`cardBg:   ${CORE_PALETTE.cardBg}    primaryMu:  ${CORE_PALETTE.primaryMuted}  red:    ${SIGNALS.red}`,
		`surfaceBg:${CORE_PALETTE.surfaceBg}    primaryBr:  ${CORE_PALETTE.primaryBright}  orange: ${SIGNALS.orange}`,
		`                                          yellow: ${SIGNALS.yellow}`,
		"",
		"TEXT                 BORDERS",
		`fg:       ${CORE_PALETTE.fg}    border:     ${CORE_PALETTE.borderColor}`,
		`mutedFg:  ${CORE_PALETTE.mutedFg}    borderDim:  ${CORE_PALETTE.borderDim}`,
		`dimFg:    ${CORE_PALETTE.dimFg}`,
		"",
		"EXCALIDRAW SEMANTICS (fill / stroke)",
		`primary:   ${EXCALIDRAW_SEMANTICS.primary.fill} / ${EXCALIDRAW_SEMANTICS.primary.stroke}    start:     ${EXCALIDRAW_SEMANTICS.start.fill} / ${EXCALIDRAW_SEMANTICS.start.stroke}`,
		`secondary: ${EXCALIDRAW_SEMANTICS.secondary.fill} / ${EXCALIDRAW_SEMANTICS.secondary.stroke}    end:       ${EXCALIDRAW_SEMANTICS.end.fill} / ${EXCALIDRAW_SEMANTICS.end.stroke}`,
		`decision:  ${EXCALIDRAW_SEMANTICS.decision.fill} / ${EXCALIDRAW_SEMANTICS.decision.stroke}    ai:        ${EXCALIDRAW_SEMANTICS.ai.fill} / ${EXCALIDRAW_SEMANTICS.ai.stroke}`,
		`warning:   ${EXCALIDRAW_SEMANTICS.warning.fill} / ${EXCALIDRAW_SEMANTICS.warning.stroke}    error:     ${EXCALIDRAW_SEMANTICS.error.fill} / ${EXCALIDRAW_SEMANTICS.error.stroke}`,
		`evidence:  ${EXCALIDRAW_SEMANTICS.evidence.fill} / ${EXCALIDRAW_SEMANTICS.evidence.stroke}    inactive:  ${EXCALIDRAW_SEMANTICS.inactive.fill} / ${EXCALIDRAW_SEMANTICS.inactive.stroke}`,
		"```",
		"",
		"`/style palette` — render visual swatch  ·  `/style d2` — D2 style template",
		"`/style excalidraw` — semantic palette table  ·  `/style check <file>` — audit colors",
	].join("\n");
}

function excalidrawTable(): string {
	const rows = Object.entries(EXCALIDRAW_SEMANTICS).map(
		([name, { fill, stroke, use }]) => `| \`${name}\` | \`${fill}\` | \`${stroke}\` | ${use} |`
	);
	return [
		"**Excalidraw Semantic Palette**",
		"",
		"| Purpose | Fill | Stroke | Use |",
		"|---------|------|--------|-----|",
		...rows,
		"",
		"Text on light fills (`start`, `end`, `decision`, `warning`, `error`, `inactive`): use `#374151`",
		"Text on dark fills (`primary`, `secondary`, `evidence`): use `#ffffff`",
	].join("\n");
}

function d2Template(): string {
	return [
		"**D2 Verdant Style Template**",
		"",
		"Copy and adapt for your diagrams. Renders with `--theme 200 --layout elk`:",
		"",
		"```d2",
		D2_STYLE_TEMPLATE,
		"```",
	].join("\n");
}

function auditColors(filePath: string): string {
	const fs = require("node:fs");
	let content: string;
	try {
		content = fs.readFileSync(filePath, "utf-8");
	} catch {
		return `❌ Could not read file: \`${filePath}\``;
	}

	const hexPattern = /#[0-9a-fA-F]{6}\b/g;
	const found = new Set<string>();
	let match: RegExpExecArray | null;
	while ((match = hexPattern.exec(content)) !== null) {
		found.add(match[0].toLowerCase());
	}

	if (found.size === 0) {
		return `✅ No hex colors found in \`${filePath}\``;
	}

	const onPalette: string[] = [];
	const offPalette: string[] = [];
	const tokensByHex = new Map<string, string[]>();
	for (const [name, hex] of Object.entries(ALL_TOKENS)) {
		const h = hex.toLowerCase();
		if (!tokensByHex.has(h)) tokensByHex.set(h, []);
		tokensByHex.get(h)!.push(name);
	}

	for (const hex of [...found].sort()) {
		const tokens = tokensByHex.get(hex);
		if (tokens) {
			onPalette.push(`  ✅ \`${hex}\` → ${tokens.join(", ")}`);
		} else {
			offPalette.push(`  ⚠️  \`${hex}\` — **off-palette**`);
		}
	}

	const lines = [`**Color Audit: \`${filePath}\`**`, "", `Found ${found.size} unique hex colors:`, ""];
	if (onPalette.length) lines.push("**On-palette:**", ...onPalette, "");
	if (offPalette.length) lines.push("**Off-palette:**", ...offPalette, "");
	return lines.join("\n");
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function styleExtension(pi: ExtensionAPI) {
	pi.registerCommand("style", {
		description: "Verdant design system (usage: /style [palette|d2|excalidraw|check <file>])",
		getArgumentCompletions: (prefix: string) => {
			const parts = prefix.split(/\s+/);
			if (parts.length <= 1) {
				const subs: Array<{ value: string; label: string; description: string }> = [
					{ value: "palette", label: "palette", description: "Render visual swatch" },
					{ value: "d2", label: "d2", description: "D2 style template" },
					{ value: "excalidraw", label: "excalidraw", description: "Semantic palette table" },
					{ value: "check", label: "check", description: "Audit file colors" },
				];
				const filtered = subs.filter(s => s.value.startsWith(parts[0] || ""));
				return filtered.length > 0 ? filtered : null;
			}
			return null;
		},
		handler: async (args, _ctx) => {
			const trimmed = (args || "").trim();
			const [subcommand, ...rest] = trimmed.split(/\s+/);

			let output: string;

			switch (subcommand || "") {
				case "":
					output = quickRef();
					break;
				case "palette":
					// Delegate to agent to render via render_diagram tool (D2)
					pi.sendUserMessage(
						"Render a D2 diagram showing the Verdant palette as a visual swatch. " +
						"Use the style skill's color tokens. Group into containers: Core (bg/cardBg/surfaceBg + primary/primaryMuted/primaryBright + fg/mutedFg/dimFg), " +
						"Signals (green/red/orange/yellow), and Borders (borderColor/borderDim). " +
						"Style each node with its actual hex value as fill color, appropriate font-color for contrast, " +
						"and label it with token name + hex value. Use D2 style blocks.",
						{ deliverAs: "followUp" },
					);
					return;
				case "d2":
					output = d2Template();
					break;
				case "excalidraw":
					output = excalidrawTable();
					break;
				case "check": {
					const filePath = rest.join(" ");
					if (!filePath) {
						output = "Usage: `/style check <file path>`";
					} else {
						output = auditColors(filePath);
					}
					break;
				}
				default:
					// Treat as a question — delegate to agent with style skill context
					pi.sendUserMessage(
						`The user asked about the style system: "${trimmed}". ` +
						`Answer using the style skill (Verdant design system). Load /skill:style if needed.`,
						{ deliverAs: "followUp" },
					);
					return;
			}

			pi.sendMessage({ customType: "view", content: output, display: true });
		},
	});
}
