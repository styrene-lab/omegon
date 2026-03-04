// @secret HF_TOKEN "HuggingFace token (gated model access for FLUX.1)"
// @config DIFFUSION_CLI_DIR "Path to uv project with mflux installed" [default: ~/diffusion-cli]
// @config PI_VISUALS_DIR "Output directory for generated images and diagrams" [default: ~/.pi/visuals]
// @config EXCALIDRAW_RENDER_DIR "Path to Excalidraw render pipeline (uv + playwright)" [default: <pi-kit>/extensions/render/excalidraw-renderer]

/**
 * render — Visual rendering extension for pi
 *
 * Provides three tools:
 *   - generate_image_local: AI image generation via FLUX.1 (mflux, Apple Silicon MLX)
 *   - render_diagram: D2 diagram rendering via d2 CLI
 *   - render_excalidraw: Excalidraw JSON → PNG via Playwright + headless Chromium
 *
 * All tools save output to ~/.pi/visuals/ for persistence across sessions.
 *
 * Requirements:
 *   generate_image_local:
 *     - Apple Silicon Mac with sufficient RAM (16GB+ quantized, 32GB+ full)
 *     - uv + mflux installed (set DIFFUSION_CLI_DIR or use ~/diffusion-cli default)
 *     - HuggingFace token for gated models: /secrets configure HF_TOKEN
 *   render_diagram:
 *     - d2 CLI (installed via Nix or brew)
 *     - Falls back to syntax-highlighted source if d2 is not installed
 *   render_excalidraw:
 *     - uv + playwright + chromium
 *     - First-time setup: cd <EXCALIDRAW_RENDER_DIR> && uv sync && uv run playwright install chromium
 */

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join, basename } from "node:path";
import { mkdtempSync } from "node:fs";
import { StringEnum } from "@mariozechner/pi-ai";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";

// ---------------------------------------------------------------------------
// Shared output directory
// ---------------------------------------------------------------------------

const VISUALS_DIR = process.env.PI_VISUALS_DIR || join(homedir(), ".pi", "visuals");

function ensureVisualsDir() {
	mkdirSync(VISUALS_DIR, { recursive: true });
}

function visualsPath(filename: string): string {
	ensureVisualsDir();
	return join(VISUALS_DIR, filename);
}

function timestamp(): string {
	return new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
}

function hasCmd(cmd: string): boolean {
	try {
		execSync(`which ${cmd}`, { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

// ---------------------------------------------------------------------------
// Diffusion config
// ---------------------------------------------------------------------------

const DIFFUSION_CLI_DIR = process.env.DIFFUSION_CLI_DIR || join(homedir(), "diffusion-cli");

// Excalidraw renderer lives alongside this extension.
const EXCALIDRAW_RENDER_DIR = process.env.EXCALIDRAW_RENDER_DIR ||
	join(import.meta.dirname ?? __dirname, "excalidraw-renderer");

const PRESETS = ["schnell", "dev", "dev-fast", "diagram", "portrait", "wide"] as const;

const PRESET_DESCRIPTIONS: Record<(typeof PRESETS)[number], string> = {
	schnell:    "FLUX.1-schnell — fastest, ~10s, 4 steps",
	dev:        "FLUX.1-dev — high quality, ~60s, 25 steps",
	"dev-fast": "FLUX.1-dev — balanced, ~30s, 12 steps",
	diagram:    "Optimized for technical diagrams (1024x768)",
	portrait:   "Portrait orientation (768x1024), high quality",
	wide:       "Cinematic wide (1344x768), fast",
};

const PRESET_DEFAULTS: Record<string, { model: string; steps: number; guidance: number; width: number; height: number }> = {
	schnell:    { model: "schnell", steps: 4,  guidance: 0.0, width: 1024, height: 1024 },
	dev:        { model: "dev",     steps: 25, guidance: 3.5, width: 1024, height: 1024 },
	"dev-fast": { model: "dev",     steps: 12, guidance: 3.5, width: 1024, height: 1024 },
	diagram:    { model: "schnell", steps: 4,  guidance: 0.0, width: 1024, height: 768  },
	portrait:   { model: "dev",     steps: 25, guidance: 3.5, width: 768,  height: 1024 },
	wide:       { model: "schnell", steps: 4,  guidance: 0.0, width: 1344, height: 768  },
};

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function renderExtension(pi: ExtensionAPI) {

	// ------------------------------------------------------------------
	// generate_image_local — FLUX.1 via mflux on Apple Silicon
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "generate_image_local",
		label: "Generate Image (Local)",
		description: [
			"Generate an image locally on Apple Silicon using FLUX.1 via MLX.",
			"Returns the generated image inline. Runs entirely on-device, no cloud API needed.",
			"Output is saved to ~/.pi/visuals/ for persistence.",
			"",
			"Presets:",
			...PRESETS.map((p) => `  ${p}: ${PRESET_DESCRIPTIONS[p]}`),
			"",
			"For technical diagrams, use the 'diagram' preset.",
			"For fast iteration, use 'schnell'. For quality, use 'dev'.",
			"Quantize to 4 or 8 bits to reduce memory usage and speed up generation.",
		].join("\n"),
		promptSnippet: "Generate images locally via FLUX.1 on Apple Silicon (no cloud API)",
		promptGuidelines: [
			"Use 'diagram' preset for technical diagrams, 'schnell' for fast iteration, 'dev' for quality",
			"Quantize to 4 or 8 bits to reduce memory usage and speed up generation",
		],

		parameters: Type.Object({
			prompt:    Type.String({ description: "Text prompt describing the image to generate" }),
			preset:    Type.Optional(StringEnum(PRESETS, { description: "Generation preset. Default: schnell" })),
			width:     Type.Optional(Type.Number({ description: "Image width in pixels (multiple of 64)" })),
			height:    Type.Optional(Type.Number({ description: "Image height in pixels (multiple of 64)" })),
			steps:     Type.Optional(Type.Number({ description: "Number of diffusion steps" })),
			guidance:  Type.Optional(Type.Number({ description: "Classifier-free guidance scale" })),
			seed:      Type.Optional(Type.Number({ description: "Random seed for reproducibility" })),
			quantize:  Type.Optional(StringEnum(["3", "4", "5", "6", "8"] as const, { description: "Quantization bits (lower = faster/less VRAM)" })),
			model:     Type.Optional(Type.String({ description: "Override model (HuggingFace repo or local path)" })),
		}),

		async execute(_toolCallId, params, signal, onUpdate, _ctx) {
			if (!existsSync(DIFFUSION_CLI_DIR)) {
				throw new Error(
					`diffusion-cli not found at ${DIFFUSION_CLI_DIR}. ` +
					`Set it up with: uv init ~/diffusion-cli && cd ~/diffusion-cli && uv add mflux\n` +
					`Or set DIFFUSION_CLI_DIR to point to an existing mflux project.`
				);
			}

			const preset = params.preset || "schnell";
			const defaults = PRESET_DEFAULTS[preset] || PRESET_DEFAULTS.schnell;
			const modelName = params.model || defaults.model;

			const mfluxBin = join(DIFFUSION_CLI_DIR, ".venv", "bin", "mflux-generate");
			const slug = params.prompt.slice(0, 40).replace(/[^a-zA-Z0-9]/g, "_");
			const outputPath = visualsPath(`${timestamp()}_${slug}.png`);

			const args = [
				"--model",    modelName,
				"--prompt",   params.prompt,
				"--width",    String(params.width    || defaults.width),
				"--height",   String(params.height   || defaults.height),
				"--steps",    String(params.steps    || defaults.steps),
				"--guidance", String(params.guidance ?? defaults.guidance),
				"--output",   outputPath,
				"--metadata",
			];
			if (params.seed      !== undefined) args.push("--seed",     String(params.seed));
			if (params.quantize)                args.push("--quantize", params.quantize);

			onUpdate?.({
				content: [{ type: "text", text: `Generating with ${modelName} (${preset})…` }],
				details: { preset, model: modelName },
			});

			const startTime = Date.now();
			const result = await pi.exec(mfluxBin, args, { signal, timeout: 600_000 });
			const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);

			if (result.code !== 0) {
				const stderr = result.stderr || "";
				if (stderr.includes("GatedRepoError") || stderr.includes("401")) {
					throw new Error(
						"HuggingFace authentication required. The model is gated.\n" +
						"1. Accept the license at https://huggingface.co/black-forest-labs/FLUX.1-schnell\n" +
						"2. Run: /secrets configure HF_TOKEN (paste your HuggingFace access token)\n" +
						"3. Retry the generation."
					);
				}
				throw new Error(`mflux-generate failed (exit ${result.code}):\n${stderr.slice(-1500)}`);
			}

			if (!existsSync(outputPath)) {
				throw new Error(`Image was not created at ${outputPath}. Stdout: ${result.stdout?.slice(-500)}`);
			}

			const imageBuffer = await readFile(outputPath);
			const base64Data = imageBuffer.toString("base64");

			const w = params.width  || defaults.width;
			const h = params.height || defaults.height;
			const summary = [
				`Generated in ${elapsed}s via mflux/${modelName} (${preset}).`,
				`Resolution: ${w}×${h}`,
				params.seed     !== undefined ? `Seed: ${params.seed}` : "",
				params.quantize                ? `Quantized: ${params.quantize}-bit` : "",
				`Saved: ${outputPath}`,
			].filter(Boolean).join("  ·  ");

			return {
				content: [
					{ type: "text",  text: summary },
					{ type: "image", data: base64Data, mimeType: "image/png" },
				],
				details: { preset, model: modelName, elapsed: Number(elapsed), outputPath, width: w, height: h, seed: params.seed, quantize: params.quantize },
			};
		},
	});

	// ------------------------------------------------------------------
	// render_diagram — D2 code → inline PNG via d2 CLI
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "render_diagram",
		label: "Render Diagram",
		description:
			"Render a D2 diagram as an inline PNG image in the terminal. " +
			"D2 is a modern declarative diagramming language (https://d2lang.com). " +
			"Use for architecture diagrams, flowcharts, ER diagrams, sequence diagrams, " +
			"class diagrams, state machines, and any structural diagram. " +
			"Output is saved to ~/.pi/visuals/ for persistence. " +
			"Requires d2 CLI (installed via Nix or brew).",
		promptSnippet: "Render D2 diagrams as inline images (flowcharts, ER, sequence, architecture, etc.)",
		promptGuidelines: [
			"Use D2 syntax, NOT Mermaid. D2 reference: https://d2lang.com/tour/intro",
			"Use --theme 200 (dark) and --layout elk for best results",
			"Apply Verdant semantic colors via style blocks: fill, stroke, font-color",
		],
		parameters: Type.Object({
			code:   Type.String({ description: "D2 diagram source code" }),
			title:  Type.Optional(Type.String({ description: "Optional title for the diagram" })),
			layout: Type.Optional(StringEnum(["dagre", "elk"] as const, { description: "Layout engine (default: elk)" })),
			theme:  Type.Optional(Type.Number({ description: "D2 theme ID (default: 200 = dark)" })),
			sketch: Type.Optional(Type.Boolean({ description: "Sketch/hand-drawn mode (default: false)" })),
		}),
		async execute(_toolCallId, params, signal, onUpdate, _ctx) {
			if (!hasCmd("d2")) {
				throw new Error(
					"d2 CLI not found. Install via Nix (nix profile install nixpkgs#d2) " +
					"or brew (brew install d2)."
				);
			}

			const slug = (params.title || "diagram").replace(/[^a-zA-Z0-9]/g, "_").slice(0, 40);
			const d2Path  = visualsPath(`${timestamp()}_${slug}.d2`);
			const outPng  = d2Path.replace(/\.d2$/, ".png");
			writeFileSync(d2Path, params.code, "utf-8");

			const titlePrefix = params.title ? `# ${params.title}\n\n` : "";
			const layout = params.layout ?? "elk";
			const theme = params.theme ?? 200; // dark theme

			const args = [
				"-l", layout,
				"-t", String(theme),
				"--pad", "40",
			];
			if (params.sketch) args.push("--sketch");
			args.push(d2Path, outPng);

			onUpdate?.({
				content: [{ type: "text", text: `Rendering D2 diagram (${layout})…` }],
				details: { d2Path, layout, theme },
			});

			const startTime = Date.now();
			try {
				const result = await pi.exec("d2", args, { signal, timeout: 30_000 });
				const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);

				if (result.code !== 0) {
					const stderr = result.stderr || "";
					throw new Error(`d2 failed (exit ${result.code}):\n${stderr.slice(-1500)}`);
				}

				if (!existsSync(outPng) || statSync(outPng).size === 0) {
					throw new Error(`d2 produced no output at ${outPng}`);
				}

				const data = readFileSync(outPng).toString("base64");
				return {
					content: [
						{ type: "text",  text: `${titlePrefix}📊 D2 (${layout}, ${elapsed}s)  ·  Saved: ${outPng}` },
						{ type: "image", data, mimeType: "image/png" },
					],
					details: { rendered: true, d2Path, pngPath: outPng, layout, theme, elapsed: Number(elapsed) },
				};
			} catch (err: any) {
				// Fallback: show source
				return {
					content: [{
						type: "text",
						text: `${titlePrefix}📊 D2 source (render failed: ${err.message})  ·  Saved: ${d2Path}\n\n\`\`\`d2\n${params.code}\n\`\`\``,
					}],
					details: { rendered: false, d2Path, error: err.message },
				};
			}
		},
	});

	// ------------------------------------------------------------------
	// render_excalidraw — Excalidraw JSON → PNG via Playwright
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "render_excalidraw",
		label: "Render Excalidraw",
		description:
			"Render an .excalidraw JSON file to PNG using Playwright + headless Chromium. " +
			"Takes a path to an existing .excalidraw file, renders it, and returns the PNG inline. " +
			"Output is saved to ~/.pi/visuals/. " +
			"First-time setup: cd <render_dir> && uv sync && uv run playwright install chromium",
		promptSnippet: "Render .excalidraw JSON files to inline PNG images",
		promptGuidelines: [
			"Include ALL necessary context in the prompt — the local model cannot see conversation history or access tools",
			"Use Excalidraw for freeform visual arguments where spatial layout matters — not for structural diagrams (use D2 for those)",
			"Write complete .excalidraw JSON with the element template: roughness 0, fillStyle solid, fontFamily 3 (Cascadia), viewBackgroundColor #ffffff",
			"Every element id must be unique; index values must be alphabetically ordered (a0, a1, a2...)",
			"If a text element has containerId, the container must list that text in boundElements",
			"Arrow points are relative to the arrow's x/y — first point is always [0, 0]",
			"Apply Verdant semantic colors: primary (#3b82f6/#1e3a5f), start (#fed7aa/#c2410c), end (#a7f3d0/#047857), decision (#fef3c7/#b45309), ai (#ddd6fe/#6d28d9), evidence (#1e293b/#334155)",
			"Text on dark fills: #ffffff. Text on light fills: #374151",
			"Diagrams argue, not display — the shape should mirror the concept (fan-out for one-to-many, convergence for aggregation, timeline for sequences)",
		],
		parameters: Type.Object({
			path:   Type.String({ description: "Path to .excalidraw JSON file to render" }),
			scale:  Type.Optional(Type.Number({ description: "Device scale factor (default: 2)" })),
			title:  Type.Optional(Type.String({ description: "Optional title for the output" })),
		}),
		async execute(_toolCallId, params, signal, onUpdate, _ctx) {
			const excalidrawPath = params.path;

			if (!existsSync(excalidrawPath)) {
				throw new Error(`File not found: ${excalidrawPath}`);
			}

			const renderScript = join(EXCALIDRAW_RENDER_DIR, "render_excalidraw.py");
			if (!existsSync(renderScript)) {
				throw new Error(
					`Excalidraw render script not found at ${renderScript}.\n` +
					`Expected at: ${EXCALIDRAW_RENDER_DIR}/render_excalidraw.py`
				);
			}

			// Check if uv project is set up
			const uvLock = join(EXCALIDRAW_RENDER_DIR, "uv.lock");
			if (!existsSync(uvLock)) {
				throw new Error(
					`Excalidraw renderer not set up. Run:\n` +
					`  cd ${EXCALIDRAW_RENDER_DIR} && uv sync && uv run playwright install chromium`
				);
			}

			const scale = params.scale ?? 2;
			const slug = (params.title || basename(excalidrawPath, ".excalidraw")).replace(/[^a-zA-Z0-9]/g, "_").slice(0, 40);
			const outPng = visualsPath(`${timestamp()}_${slug}.png`);

			onUpdate?.({
				content: [{ type: "text", text: `Rendering ${basename(excalidrawPath)}…` }],
				details: { excalidrawPath },
			});

			try {
				const result = await pi.exec(
					"uv",
					["run", "python", renderScript, excalidrawPath, "--output", outPng, "--scale", String(scale)],
					{ signal, timeout: 60_000, cwd: EXCALIDRAW_RENDER_DIR },
				);

				if (result.code !== 0) {
					const stderr = result.stderr || "";
					if (stderr.includes("playwright not installed") || stderr.includes("Chromium not installed")) {
						throw new Error(
							`Excalidraw renderer needs setup:\n` +
							`  cd ${EXCALIDRAW_RENDER_DIR} && uv sync && uv run playwright install chromium`
						);
					}
					throw new Error(`Render failed (exit ${result.code}):\n${stderr.slice(-1500)}`);
				}

				if (!existsSync(outPng) || statSync(outPng).size === 0) {
					throw new Error(`Render produced no output at ${outPng}`);
				}

				const data = readFileSync(outPng).toString("base64");
				const titlePrefix = params.title ? `# ${params.title}\n\n` : "";

				return {
					content: [
						{ type: "text",  text: `${titlePrefix}📐 Excalidraw  ·  Saved: ${outPng}` },
						{ type: "image", data, mimeType: "image/png" },
					],
					details: { rendered: true, excalidrawPath, pngPath: outPng, scale },
				};
			} catch (err: any) {
				if (err.message?.includes("renderer needs setup") || err.message?.includes("not set up")) {
					throw err;
				}
				throw new Error(`Excalidraw render failed: ${err.message}`);
			}
		},
	});

	// ------------------------------------------------------------------
	// /render command — quick image generation shortcut
	// ------------------------------------------------------------------
	pi.registerCommand("render", {
		description: "Generate an image locally (usage: /render <prompt>)",
		handler: async (args, _ctx) => {
			if (!args?.trim()) {
				// Show status instead of error
				const mfluxOk = existsSync(join(DIFFUSION_CLI_DIR, ".venv", "bin", "mflux-generate"));
				const d2Ok    = hasCmd("d2");
				const excaliOk = existsSync(join(EXCALIDRAW_RENDER_DIR, "uv.lock"));
				const status = [
					`**Visual generation status**`,
					``,
					`FLUX.1 (generate_image_local): ${mfluxOk ? "✅ ready" : `❌ not found — set up ${DIFFUSION_CLI_DIR}`}`,
					`D2 (render_diagram): ${d2Ok ? "✅ ready" : "❌ not installed — \`nix profile install nixpkgs#d2\` or \`brew install d2\`"}`,
					`Excalidraw (render_excalidraw): ${excaliOk ? "✅ ready" : `⚠️  not set up — \`cd ${EXCALIDRAW_RENDER_DIR} && uv sync && uv run playwright install chromium\``}`,
					`Output directory: \`${VISUALS_DIR}\``,
					``,
					`Usage: \`/render <prompt>\``,
				].join("\n");
				pi.sendMessage({ customType: "view", content: status, display: true });
				return;
			}

			pi.sendUserMessage(
				`Use the generate_image_local tool to create an image with this prompt: ${args}`,
				{ deliverAs: "followUp" }
			);
		},
	});
}
