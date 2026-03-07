/**
 * /view and /edit — Inline file viewer and editor launcher for pi TUI
 *
 * /view renders files inline with syntax highlighting, image rendering, etc.
 * /edit opens files in $EDITOR (vim, nvim, etc.)
 *
 * Supported formats:
 *   Images:    jpg, jpeg, png, gif, webp, svg, bmp, tiff, ico, heic
 *   Documents: pdf, docx, xlsx, pptx, odt, epub, html, csv, tsv, rtf
 *   Diagrams:  D2 (.d2)
 *   Data:      json, yaml, xml, toml
 *   Text:      md, txt, and any text file (syntax-highlighted)
 *
 * Dependencies:
 *   - poppler (pdftotext, pdftoppm) — PDF rendering
 *   - pandoc — document conversion
 *   - d2 — D2 diagram rendering
 */

import { execSync, execFileSync, spawnSync } from "node:child_process";
// Note: execSync retained solely for hasCmd() which takes hardcoded strings only
import {
	existsSync, readFileSync, statSync, mkdtempSync,
	readdirSync, accessSync, constants,
} from "node:fs";
import { basename, extname, resolve, join, relative } from "node:path";
import { tmpdir } from "node:os";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";
import { Type } from "@sinclair/typebox";
import { resolveUri, loadConfig, osc8Link } from "./uri-resolver.js";
import { getMdservePort } from "../vault/index.ts";

// ---------------------------------------------------------------------------
// Format classification
// ---------------------------------------------------------------------------

const IMAGE_EXTS = new Set([
	".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp",
	".tiff", ".tif", ".ico", ".heic", ".avif",
]);
const SVG_EXTS = new Set([".svg"]);
const PDF_EXTS = new Set([".pdf"]);
const PANDOC_EXTS = new Set([
	".docx", ".xlsx", ".pptx", ".odt", ".epub",
	".html", ".htm", ".rtf", ".rst", ".textile",
	".mediawiki", ".org", ".opml", ".csv", ".tsv", ".bib",
]);
const DIAGRAM_EXTS = new Set([".d2"]);
const DATA_EXTS = new Set([".json", ".yaml", ".yml", ".xml", ".toml"]);
const MARKDOWN_EXTS = new Set([".md", ".markdown", ".mdx"]);

// Files that should open in $EDITOR rather than inline view
const EDITABLE_EXTS = new Set([
	...DATA_EXTS, ...MARKDOWN_EXTS,
	".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs",
	".py", ".rs", ".go", ".c", ".cpp", ".h", ".hpp",
	".java", ".kt", ".swift", ".rb", ".lua", ".zig",
	".sh", ".bash", ".zsh", ".fish",
	".sql", ".css", ".scss", ".less",
	".tf", ".hcl", ".nix", ".dhall",
	".txt", ".cfg", ".ini", ".conf", ".env",
	".makefile", ".dockerfile",
]);

type FileKind = "image" | "svg" | "pdf" | "pandoc" | "diagram" | "data" | "markdown" | "text" | "binary";

function classifyFile(filePath: string): FileKind {
	const ext = extname(filePath).toLowerCase();
	if (IMAGE_EXTS.has(ext)) return "image";
	if (SVG_EXTS.has(ext)) return "svg";
	if (PDF_EXTS.has(ext)) return "pdf";
	if (PANDOC_EXTS.has(ext)) return "pandoc";
	if (DIAGRAM_EXTS.has(ext)) return "diagram";
	if (DATA_EXTS.has(ext)) return "data";
	if (MARKDOWN_EXTS.has(ext)) return "markdown";

	// Check if file is text by reading first chunk
	try {
		const buf = Buffer.alloc(512);
		const fd = require("node:fs").openSync(filePath, "r");
		const bytesRead = require("node:fs").readSync(fd, buf, 0, 512, 0);
		require("node:fs").closeSync(fd);
		// If the first 512 bytes contain a null byte, treat as binary
		for (let i = 0; i < bytesRead; i++) {
			if (buf[i] === 0) return "binary";
		}
	} catch { /* treat as text */ }
	return "text";
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

function hasCmd(cmd: string): boolean {
	try {
		execSync(`which ${cmd}`, { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

/**
 * Run a command with argument array — no shell interpolation, safe for untrusted paths.
 */
function runSafe(cmd: string, args: string[], opts?: { timeout?: number }): string {
	return execFileSync(cmd, args, {
		encoding: "utf-8",
		maxBuffer: 10 * 1024 * 1024,
		timeout: opts?.timeout ?? 30_000,
	}).trim();
}

function fileSizeStr(bytes: number): string {
	if (bytes < 1024) return `${bytes}B`;
	if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
	if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)}MB`;
	return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)}GB`;
}

function mimeFromExt(ext: string): string {
	const map: Record<string, string> = {
		".jpg": "image/jpeg", ".jpeg": "image/jpeg",
		".png": "image/png", ".gif": "image/gif",
		".webp": "image/webp", ".bmp": "image/bmp",
		".tiff": "image/tiff", ".tif": "image/tiff",
		".ico": "image/x-icon", ".heic": "image/heic",
		".avif": "image/avif", ".svg": "image/svg+xml",
	};
	return map[ext.toLowerCase()] ?? "image/png";
}

function modifiedAgo(mtime: Date): string {
	const diff = Date.now() - mtime.getTime();
	const secs = Math.floor(diff / 1000);
	if (secs < 60) return "just now";
	const mins = Math.floor(secs / 60);
	if (mins < 60) return `${mins}m ago`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h ago`;
	const days = Math.floor(hours / 24);
	if (days < 30) return `${days}d ago`;
	return mtime.toLocaleDateString();
}

function fileHeader(filePath: string, icon: string, extra?: string, uri?: string): string {
	const stat = statSync(filePath);
	const name = basename(filePath);
	const label = uri ? osc8Link(uri, `${icon} ${name}`) : `${icon} ${name}`;
	const parts = [
		label,
		fileSizeStr(stat.size),
		`modified ${modifiedAgo(stat.mtime)}`,
	];
	if (extra) parts.push(extra);
	return parts.join("  ·  ");
}

// ---------------------------------------------------------------------------
// Content type for results
// ---------------------------------------------------------------------------

type ContentPart =
	| { type: "text"; text: string }
	| { type: "image"; data: string; mimeType: string };

interface ViewResult {
	content: ContentPart[];
	details: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// Image dimensions (best-effort via sips on macOS or file command)
// ---------------------------------------------------------------------------

function getImageDims(filePath: string): string | undefined {
	try {
		const out = runSafe("sips", ["-g", "pixelWidth", "-g", "pixelHeight", filePath]);
		const w = out.match(/pixelWidth:\s+(\d+)/)?.[1];
		const h = out.match(/pixelHeight:\s+(\d+)/)?.[1];
		if (w && h) return `${w}×${h}`;
	} catch { /* ignore */ }
	try {
		const out = runSafe("file", [filePath]);
		const m = out.match(/(\d+)\s*x\s*(\d+)/);
		if (m) return `${m[1]}×${m[2]}`;
	} catch { /* ignore */ }
	return undefined;
}

// ---------------------------------------------------------------------------
// Renderers
// ---------------------------------------------------------------------------

function viewImage(filePath: string, uri?: string): ViewResult {
	const data = readFileSync(filePath).toString("base64");
	const ext = extname(filePath).toLowerCase();
	const mime = mimeFromExt(ext);
	const dims = getImageDims(filePath);
	return {
		content: [
			{ type: "text", text: fileHeader(filePath, "📷", dims, uri) },
			{ type: "image", data, mimeType: mime },
		],
		details: { kind: "image", path: filePath, dimensions: dims },
	};
}

function viewSvg(filePath: string, uri?: string): ViewResult {
	const tmp = mkdtempSync(join(tmpdir(), "pi-view-"));
	const outPng = join(tmp, "out.png");
	let converted = false;

	if (hasCmd("rsvg-convert")) {
		try { runSafe("rsvg-convert", [filePath, "-o", outPng]); converted = true; } catch {}
	}
	if (!converted) {
		try {
			runSafe("sips", ["-s", "format", "png", filePath, "--out", outPng]);
			converted = existsSync(outPng) && statSync(outPng).size > 0;
		} catch {}
	}

	if (converted && existsSync(outPng)) {
		const data = readFileSync(outPng).toString("base64");
		return {
			content: [
				{ type: "text", text: fileHeader(filePath, "🎨", "SVG → PNG", uri) },
				{ type: "image", data, mimeType: "image/png" },
			],
			details: { kind: "svg", path: filePath, rendered: true },
		};
	}

	// Fall back to source with syntax highlighting
	const src = readFileSync(filePath, "utf-8");
	const preview = src.length > 5000 ? src.slice(0, 5000) + "\n… (truncated)" : src;
	return {
		content: [{ type: "text", text: `${fileHeader(filePath, "🎨", "SVG source", uri)}\n\n\`\`\`xml\n${preview}\n\`\`\`` }],
		details: { kind: "svg", path: filePath, rendered: false },
	};
}

function viewPdf(filePath: string, page?: number, uri?: string): ViewResult {
	const content: ContentPart[] = [];

	// Page count
	let pageCount = 0;
	try {
		const info = runSafe("pdfinfo", [filePath]);
		const m = info.match(/Pages:\s+(\d+)/);
		if (m) pageCount = parseInt(m[1], 10);
	} catch {}

	const extra = pageCount > 0 ? `${pageCount} pages` : undefined;
	content.push({ type: "text", text: fileHeader(filePath, "📄", extra, uri) });

	if (hasCmd("pdftoppm")) {
		const tmp = mkdtempSync(join(tmpdir(), "pi-view-pdf-"));
		const first = page ?? 1;
		const last = page ?? Math.min(pageCount || 1, 3);

		try {
			runSafe("pdftoppm", ["-png", "-r", "200", "-f", String(first), "-l", String(last), filePath, join(tmp, "page")]);
			const pages = readdirSync(tmp).filter(f => f.endsWith(".png")).sort();

			for (let i = 0; i < pages.length; i++) {
				const pageNum = first + i;
				content.push({ type: "text", text: `\n── Page ${pageNum} ${"─".repeat(40)}` });
				const data = readFileSync(join(tmp, pages[i])).toString("base64");
				content.push({ type: "image", data, mimeType: "image/png" });
			}

			if (!page && pageCount > 3) {
				content.push({ type: "text", text: `\n📑 Showing pages 1–3 of ${pageCount}. Use \`/view ${basename(filePath)} <page>\` for a specific page.` });
			}
		} catch {
			appendPdfText(filePath, content);
		}
	} else if (hasCmd("pdftotext")) {
		appendPdfText(filePath, content);
	} else {
		content.push({ type: "text", text: "\n⚠️  Install poppler for PDF rendering: `brew install poppler`" });
	}

	return { content, details: { kind: "pdf", path: filePath, pages: pageCount } };
}

function appendPdfText(filePath: string, content: ContentPart[]) {
	try {
		const text = runSafe("pdftotext", ["-layout", filePath, "-"]);
		const preview = text.length > 8000 ? text.slice(0, 8000) + "\n… (truncated)" : text;
		content.push({ type: "text", text: `\n\`\`\`\n${preview}\n\`\`\`` });
	} catch {
		content.push({ type: "text", text: "\n(Could not extract PDF text)" });
	}
}

function viewPandoc(filePath: string, uri?: string): ViewResult {
	const name = basename(filePath);

	if (!hasCmd("pandoc")) {
		const raw = readFileSync(filePath, "utf-8");
		const preview = raw.length > 5000 ? raw.slice(0, 5000) + "\n… (truncated)" : raw;
		return {
			content: [{ type: "text", text: `${fileHeader(filePath, "📝", undefined, uri)}\n⚠️  Install pandoc for rich rendering\n\n${preview}` }],
			details: { kind: "pandoc", path: filePath, converted: false },
		};
	}

	const ext = extname(filePath).toLowerCase();
	const formatMap: Record<string, string> = {
		".docx": "docx", ".xlsx": "csv", ".pptx": "pptx",
		".odt": "odt", ".epub": "epub", ".html": "html", ".htm": "html",
		".rtf": "rtf", ".rst": "rst", ".textile": "textile",
		".mediawiki": "mediawiki", ".org": "org", ".opml": "opml",
		".csv": "csv", ".tsv": "tsv", ".bib": "biblatex",
	};
	const fmt = formatMap[ext] ?? ext.slice(1);

	try {
		const md = runSafe("pandoc", ["-f", fmt, "-t", "gfm", "--wrap=none", filePath], { timeout: 15_000 });
		const preview = md.length > 10000 ? md.slice(0, 10000) + "\n\n… (truncated)" : md;
		return {
			content: [{ type: "text", text: `${fileHeader(filePath, "📝", fmt.toUpperCase(), uri)}\n\n${preview}` }],
			details: { kind: "pandoc", path: filePath, converted: true, format: fmt },
		};
	} catch (e: any) {
		return {
			content: [{ type: "text", text: `${fileHeader(filePath, "📝", undefined, uri)}  — conversion failed: ${e.message?.slice(0, 200)}` }],
			details: { kind: "pandoc", path: filePath, converted: false, error: e.message },
		};
	}
}

function viewDiagram(filePath: string, uri?: string): ViewResult {
	const src = readFileSync(filePath, "utf-8");

	if (hasCmd("d2")) {
		const tmp = mkdtempSync(join(tmpdir(), "pi-view-d2-"));
		const outPng = join(tmp, "diagram.png");
		try {
			runSafe("d2", ["--theme", "200", "--layout", "elk", "--pad", "40", filePath, outPng], { timeout: 15_000 });
			if (existsSync(outPng) && statSync(outPng).size > 0) {
				const data = readFileSync(outPng).toString("base64");
				return {
					content: [
						{ type: "text", text: fileHeader(filePath, "📊", "D2", uri) },
						{ type: "image", data, mimeType: "image/png" },
					],
					details: { kind: "diagram", path: filePath, rendered: true },
				};
			}
		} catch {}
	}

	return {
		content: [{ type: "text", text: `${fileHeader(filePath, "📊", "D2 source", uri)}\n\n\`\`\`d2\n${src}\n\`\`\`` }],
		details: { kind: "diagram", path: filePath, rendered: false },
	};
}

function viewText(filePath: string, lang?: string, uri?: string): ViewResult {
	const ext = extname(filePath).toLowerCase();
	const raw = readFileSync(filePath, "utf-8");
	const lineCount = raw.split("\n").length;
	const preview = raw.length > 15000 ? raw.slice(0, 15000) + "\n… (truncated)" : raw;

	const language = lang ?? guessLang(filePath);
	const fence = language ? `\`\`\`${language}` : "```";

	return {
		content: [{ type: "text", text: `${fileHeader(filePath, "📄", `${lineCount} lines · ${language || "text"}`, uri)}\n\n${fence}\n${preview}\n\`\`\`` }],
		details: { kind: "text", path: filePath, language: language || undefined, lines: lineCount },
	};
}

function viewMarkdown(filePath: string, uri?: string): ViewResult {
	const raw = readFileSync(filePath, "utf-8");
	const lineCount = raw.split("\n").length;
	const preview = raw.length > 15000 ? raw.slice(0, 15000) + "\n\n… (truncated)" : raw;

	return {
		content: [{ type: "text", text: `${fileHeader(filePath, "📝", `${lineCount} lines · markdown`, uri)}\n\n${preview}` }],
		details: { kind: "markdown", path: filePath, lines: lineCount },
	};
}

function viewBinary(filePath: string, uri?: string): ViewResult {
	const stat = statSync(filePath);
	let fileType = "binary";
	try {
		fileType = runSafe("file", ["-b", filePath]).slice(0, 120);
	} catch {}

	return {
		content: [{ type: "text", text: `${fileHeader(filePath, "📦", fileType, uri)}\n\n(Binary file — cannot display inline)` }],
		details: { kind: "binary", path: filePath, fileType },
	};
}

function viewDirectory(absPath: string): ViewResult {
	const entries = readdirSync(absPath, { withFileTypes: true })
		.sort((a, b) => {
			// Directories first, then by name
			if (a.isDirectory() !== b.isDirectory()) return a.isDirectory() ? -1 : 1;
			return a.name.localeCompare(b.name);
		});

	const lines: string[] = [];
	let dirs = 0, files = 0;
	for (const e of entries) {
		if (e.isDirectory()) {
			dirs++;
			lines.push(`  📁 ${e.name}/`);
		} else {
			files++;
			const stat = statSync(join(absPath, e.name));
			lines.push(`  📄 ${e.name}  ${fileSizeStr(stat.size)}`);
		}
	}

	const summary = [dirs > 0 ? `${dirs} dirs` : null, files > 0 ? `${files} files` : null]
		.filter(Boolean).join(", ");

	return {
		content: [{ type: "text", text: `📁 ${basename(absPath)}/  (${summary})\n\n${lines.join("\n")}` }],
		details: { kind: "directory", path: absPath, dirs, files },
	};
}

// ---------------------------------------------------------------------------
// Language detection
// ---------------------------------------------------------------------------

const LANG_MAP: Record<string, string> = {
	".ts": "typescript", ".tsx": "typescript", ".mts": "typescript", ".cts": "typescript",
	".js": "javascript", ".jsx": "javascript", ".mjs": "javascript", ".cjs": "javascript",
	".py": "python", ".pyw": "python",
	".rs": "rust",
	".go": "go",
	".c": "c", ".h": "c",
	".cpp": "cpp", ".cc": "cpp", ".cxx": "cpp", ".hpp": "cpp",
	".java": "java",
	".kt": "kotlin", ".kts": "kotlin",
	".swift": "swift",
	".rb": "ruby",
	".lua": "lua",
	".zig": "zig",
	".sh": "bash", ".bash": "bash", ".zsh": "zsh", ".fish": "fish",
	".sql": "sql",
	".css": "css", ".scss": "scss", ".less": "less",
	".html": "html", ".htm": "html",
	".xml": "xml", ".xsl": "xml", ".xsd": "xml",
	".json": "json", ".jsonc": "json",
	".yaml": "yaml", ".yml": "yaml",
	".toml": "toml",
	".ini": "ini", ".cfg": "ini", ".conf": "ini",
	".dockerfile": "dockerfile",
	".tf": "hcl", ".hcl": "hcl",
	".nix": "nix",
	".md": "markdown", ".markdown": "markdown", ".mdx": "markdown",
	".r": "r", ".R": "r",
	".pl": "perl", ".pm": "perl",
	".ex": "elixir", ".exs": "elixir",
	".erl": "erlang",
	".hs": "haskell",
	".ml": "ocaml", ".mli": "ocaml",
	".clj": "clojure", ".cljs": "clojure",
	".scala": "scala",
	".dart": "dart",
	".vim": "vim",
	".proto": "protobuf",
	".graphql": "graphql", ".gql": "graphql",
};

const FILENAME_MAP: Record<string, string> = {
	"Makefile": "makefile", "makefile": "makefile", "GNUmakefile": "makefile",
	"Dockerfile": "dockerfile",
	"Jenkinsfile": "groovy",
	"Vagrantfile": "ruby",
	"Rakefile": "ruby",
	"Gemfile": "ruby",
	".gitignore": "gitignore",
	".dockerignore": "gitignore",
	".env": "bash",
	".bashrc": "bash", ".bash_profile": "bash", ".zshrc": "zsh",
};

function guessLang(filePath: string): string {
	const name = basename(filePath);
	if (FILENAME_MAP[name]) return FILENAME_MAP[name];
	const ext = extname(filePath).toLowerCase();
	return LANG_MAP[ext] ?? "";
}

// ---------------------------------------------------------------------------
// Main dispatcher
// ---------------------------------------------------------------------------

function viewFile(filePath: string, page?: number, options?: { mdservePort?: number }): ViewResult {
	const absPath = resolve(filePath);
	if (!existsSync(absPath)) {
		return {
			content: [{ type: "text", text: `❌ File not found: ${filePath}` }],
			details: { error: "not_found", path: filePath },
		};
	}

	if (statSync(absPath).isDirectory()) return viewDirectory(absPath);

	const config = loadConfig();
	const uri = resolveUri(absPath, { mdservePort: options?.mdservePort, config });

	const kind = classifyFile(absPath);
	switch (kind) {
		case "image": return viewImage(absPath, uri);
		case "svg": return viewSvg(absPath, uri);
		case "pdf": return viewPdf(absPath, page, uri);
		case "pandoc": return viewPandoc(absPath, uri);
		case "diagram": return viewDiagram(absPath, uri);
		case "data": return viewText(absPath, undefined, uri);
		case "markdown": return viewMarkdown(absPath, uri);
		case "binary": return viewBinary(absPath, uri);
		case "text": return viewText(absPath, undefined, uri);
	}
}

// ---------------------------------------------------------------------------
// Extension
// ---------------------------------------------------------------------------

export default function (pi: ExtensionAPI) {

	// ------------------------------------------------------------------
	// /view command
	// ------------------------------------------------------------------
	pi.registerCommand("view", {
		description: "View files inline — images, PDFs, docs, diagrams, code",
		handler: async (args, ctx) => {
			if (!args?.trim()) {
				ctx.ui.notify("Usage: /view <file> [page]", "warning");
				return;
			}

			const parts = args.trim().split(/\s+/);
			const filePath = parts[0];
			const page = parts[1] ? parseInt(parts[1], 10) : undefined;

			const mdservePort = getMdservePort() ?? undefined;
			const result = viewFile(filePath, page, { mdservePort });
			const textParts = result.content.filter(c => c.type === "text").map(c => (c as any).text).join("\n");
			const imageParts = result.content.filter(c => c.type === "image");

			pi.sendMessage({
				customType: "view",
				content: textParts,
				display: true,
				details: { ...result.details, images: imageParts.length > 0 ? imageParts : undefined },
			});
		},
	});

	// ------------------------------------------------------------------
	// /edit command
	// ------------------------------------------------------------------
	pi.registerCommand("edit", {
		description: "Open file in $EDITOR (vim, nvim, etc.)",
		handler: async (args, ctx) => {
			const rawArgs = (args ?? "").trim();
			if (!rawArgs) {
				ctx.ui.notify("Usage: /edit <file> [+line]", "warning");
				return;
			}

			// Parse: /edit file.ts +42  or  /edit +42 file.ts
			const parts = rawArgs.split(/\s+/);
			let filePath: string | undefined;
			let lineNum: string | undefined;

			for (const p of parts) {
				if (p.startsWith("+") && /^\+\d+$/.test(p)) {
					lineNum = p;
				} else if (!filePath) {
					filePath = p;
				}
			}

			if (!filePath) {
				ctx.ui.notify("Usage: /edit <file> [+line]", "warning");
				return;
			}

			const absPath = resolve(filePath);
			if (!existsSync(absPath)) {
				// Create new file — that's a valid editor use case
				ctx.ui.notify(`Creating new file: ${filePath}`, "info");
			}

			const editor = process.env.EDITOR || process.env.VISUAL || "vim";
			const editorArgs: string[] = [];

			// Pass +line to editors that support it (vim, nvim, nano, emacs, code, etc.)
			if (lineNum) editorArgs.push(lineNum);
			editorArgs.push(absPath);

			const result = spawnSync(editor, editorArgs, {
				stdio: "inherit",
				env: process.env,
			});

			if (result.status === 0) {
				ctx.ui.notify(`✓ Closed ${basename(absPath)}`, "info");
			} else if (result.error) {
				ctx.ui.notify(`Editor error: ${(result.error as Error).message}`, "error");
			}
		},
	});

	// ------------------------------------------------------------------
	// Custom message renderer for /view output
	// ------------------------------------------------------------------
	pi.registerMessageRenderer("view", (message, options, theme) => {
		const tui = require("@mariozechner/pi-tui");
		const { Container, Text, Image, Markdown, Spacer } = tui;

		let piAgent: any;
		try { piAgent = require("@mariozechner/pi-coding-agent"); } catch { piAgent = null; }

		const container = new Container();

		// Render text content as themed markdown
		if (message.content) {
			const mdTheme = piAgent?.getMarkdownTheme?.() ?? undefined;
			const md = new Markdown(message.content as string, 1, 0, mdTheme);
			container.addChild(md);
		}

		// Render images inline
		const images = (message.details as any)?.images;
		if (images && Array.isArray(images)) {
			for (const img of images) {
				try {
					const imageTheme = { fallbackColor: (s: string) => theme.fg("warning", s) };
					const image = new Image(img.data, img.mimeType, imageTheme, {
						maxWidthCells: 120,
						maxHeightCells: 40,
					});
					container.addChild(image);
				} catch {
					container.addChild(new Text(
						theme.fg("warning", "  ⚠️  Image rendering not supported in this terminal"),
						1, 0,
					));
				}
			}
		}

		return container;
	});

	// ------------------------------------------------------------------
	// view tool — LLM can show files inline
	// ------------------------------------------------------------------
	pi.registerTool({
		name: "view",
		label: "View",
		description:
			"View a file inline in the terminal with rich rendering. " +
			"Images (jpg/png/gif/webp/svg) render graphically. " +
			"PDFs render as page images. " +
			"Documents (docx/xlsx/pptx/epub) convert to markdown via pandoc. " +
			"Code files get syntax highlighting. " +
			"For PDFs, specify a page number to view a specific page.",
		promptSnippet: "View files inline with rich rendering (images, PDFs, docs, code)",
		parameters: Type.Object({
			path: Type.String({ description: "Path to the file to view" }),
			page: Type.Optional(Type.Number({ description: "Page number for PDFs (default: first 3 pages)" })),
		}),
		async execute(toolCallId, params, signal, onUpdate, ctx) {
			const filePath = params.path.startsWith("@") ? params.path.slice(1) : params.path;
			const mdservePort = getMdservePort() ?? undefined;
			return viewFile(filePath, params.page, { mdservePort });
		},
	});
}
