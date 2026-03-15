/**
 * Core Tool Renderers — Sci-UI rendering for Omegon's custom tools.
 *
 * Uses registerToolRenderer() to attach renderCall/renderResult
 * to tools that have no built-in rendering in pi-mono.
 *
 * IMPORTANT: Do NOT register renderers for built-in tools (read, edit, write,
 * bash, grep, find, ls). The built-in renderer provides syntax highlighting,
 * diffs, and streaming output. Registering even just renderCall causes pi to
 * take the custom renderer path, losing all built-in content rendering.
 */
import type { ExtensionAPI } from "@cwilson613/pi-coding-agent";
import { sciCall, sciOk, sciErr, sciLoading } from "./lib/sci-ui.ts";

/** Shorten a file path for display — keep last 2-3 segments. */
function shortenPath(p: string | null | undefined, maxLen = 55): string {
	if (!p) return "…";
	if (p.length <= maxLen) return p;
	const parts = p.split("/");
	// Show last 3 segments at most
	const tail = parts.slice(-3).join("/");
	return tail.length <= maxLen ? tail : "…" + p.slice(-(maxLen - 1));
}

export default function coreRenderers(pi: ExtensionAPI): void {
	// registerToolRenderer was added in pi-mono 0965ae87 — gracefully skip
	// if the published pi version doesn't have it yet.
	if (typeof (pi as any).registerToolRenderer !== "function") {
		return;
	}

	// ─── Built-in tools (read, edit, write, bash, grep, find, ls) ────────
	// NOT registered here. The built-in renderer handles:
	//   read  → syntax-highlighted code, line ranges, truncation
	//   edit  → colored diffs with context, line numbers
	//   write → syntax-highlighted content preview
	//   bash  → streaming output, metadata (cwd, duration, exit code)
	//   grep  → highlighted matches
	//   find  → directory listing
	//   ls    → directory listing with limits
	// Registering renderCall would cause pi to skip all of this.

	// ── View ──────────────────────────────────────────────────────────────
	pi.registerToolRenderer("view", {
		renderCall(args: any, theme: any) {
			const p = shortenPath(args?.path);
			const page = args?.page ? ` p${args.page}` : "";
			return sciCall("view", `${p}${page}`, theme);
		},
	});

	// ── Web Search ────────────────────────────────────────────────────────
	pi.registerToolRenderer("web_search", {
		renderCall(args: any, theme: any) {
			const query = args?.query ?? "";
			const mode = args?.mode ?? "quick";
			const display = query.length > 55 ? query.slice(0, 52) + "…" : query;
			const modeTag = mode !== "quick" ? ` [${mode}]` : "";
			return sciCall("web_search", `${display}${modeTag}`, theme);
		},
	});

	// ── Chronos ───────────────────────────────────────────────────────────
	pi.registerToolRenderer("chronos", {
		renderCall(args: any, theme: any) {
			const sub = args?.subcommand ?? "week";
			const expr = args?.expression ? ` "${args.expression}"` : "";
			return sciCall("chronos", `${sub}${expr}`, theme);
		},
	});

	// ── Render Diagram (D2) ───────────────────────────────────────────────
	pi.registerToolRenderer("render_diagram", {
		renderCall(args: any, theme: any) {
			const title = args?.title ?? "diagram";
			return sciCall("render_diagram", title, theme);
		},
	});

	// ── Render Native Diagram ─────────────────────────────────────────────
	pi.registerToolRenderer("render_native_diagram", {
		renderCall(args: any, theme: any) {
			const title = args?.title ?? "diagram";
			return sciCall("render_native_diagram", title, theme);
		},
	});

	// ── Render Excalidraw ─────────────────────────────────────────────────
	pi.registerToolRenderer("render_excalidraw", {
		renderCall(args: any, theme: any) {
			const p = shortenPath(args?.path);
			return sciCall("render_excalidraw", p, theme);
		},
	});

	// ── Generate Image Local ──────────────────────────────────────────────
	pi.registerToolRenderer("generate_image_local", {
		renderCall(args: any, theme: any) {
			const prompt = args?.prompt ?? "";
			const preset = args?.preset ?? "schnell";
			const display = prompt.length > 50 ? prompt.slice(0, 47) + "…" : prompt;
			return sciCall("generate_image_local", `${display} [${preset}]`, theme);
		},
	});

	// ── Render Composition Still ──────────────────────────────────────────
	pi.registerToolRenderer("render_composition_still", {
		renderCall(args: any, theme: any) {
			const p = shortenPath(args?.composition_path);
			const frame = args?.frame != null ? ` f${args.frame}` : "";
			return sciCall("render_composition_still", `${p}${frame}`, theme);
		},
	});

	// ── Render Composition Video ──────────────────────────────────────────
	pi.registerToolRenderer("render_composition_video", {
		renderCall(args: any, theme: any) {
			const p = shortenPath(args?.composition_path);
			const frames = args?.duration_in_frames ?? "?";
			const fmt = args?.format ?? "gif";
			return sciCall("render_composition_video", `${p} (${frames}f, ${fmt})`, theme);
		},
	});

	// ── Model Tier ────────────────────────────────────────────────────────
	pi.registerToolRenderer("set_model_tier", {
		renderCall(args: any, theme: any) {
			const tier = args?.tier ?? "?";
			return sciCall("set_model_tier", `→ ${tier}`, theme);
		},
	});

	// ── Thinking Level ────────────────────────────────────────────────────
	pi.registerToolRenderer("set_thinking_level", {
		renderCall(args: any, theme: any) {
			const level = args?.level ?? "?";
			return sciCall("set_thinking_level", `→ ${level}`, theme);
		},
	});

	// ── Ask Local Model ───────────────────────────────────────────────────
	pi.registerToolRenderer("ask_local_model", {
		renderCall(args: any, theme: any) {
			const model = args?.model ?? "auto";
			const prompt = args?.prompt ?? "";
			const display = prompt.length > 45 ? prompt.slice(0, 42) + "…" : prompt;
			return sciCall("ask_local_model", `[${model}] ${display}`, theme);
		},
	});

	// ── Manage Ollama ─────────────────────────────────────────────────────
	pi.registerToolRenderer("manage_ollama", {
		renderCall(args: any, theme: any) {
			const action = args?.action ?? "?";
			const model = args?.model ? ` ${args.model}` : "";
			return sciCall("manage_ollama", `${action}${model}`, theme);
		},
	});

	// ── List Local Models ─────────────────────────────────────────────────
	pi.registerToolRenderer("list_local_models", {
		renderCall(_args: any, theme: any) {
			return sciCall("list_local_models", "inventory", theme);
		},
	});

	// ── Switch Offline Driver ─────────────────────────────────────────────
	pi.registerToolRenderer("switch_to_offline_driver", {
		renderCall(args: any, theme: any) {
			const reason = args?.reason ?? "";
			const display = reason.length > 50 ? reason.slice(0, 47) + "…" : reason;
			return sciCall("switch_to_offline_driver", display, theme);
		},
	});

	// ── Manage Tools ──────────────────────────────────────────────────────
	pi.registerToolRenderer("manage_tools", {
		renderCall(args: any, theme: any) {
			const action = args?.action ?? "list";
			const tools = args?.tools?.join(", ") ?? "";
			const profile = args?.profile ?? "";
			const detail = tools || profile || "";
			return sciCall("manage_tools", `${action}${detail ? ` ${detail}` : ""}`, theme);
		},
	});

	// ── Whoami ────────────────────────────────────────────────────────────
	pi.registerToolRenderer("whoami", {
		renderCall(_args: any, theme: any) {
			return sciCall("whoami", "check auth", theme);
		},
	});
}
