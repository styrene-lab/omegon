/**
 * Dependency registry — declarative external dependency catalog.
 *
 * Each dep has a check function (is it available?), install hint,
 * tier (core vs optional), and the extensions that need it.
 */

import { execSync } from "node:child_process";

export type DepTier = "core" | "recommended" | "optional";

export interface Dep {
	/** Short identifier */
	id: string;
	/** Human-readable name */
	name: string;
	/** What it does in Omegon context */
	purpose: string;
	/** Which extensions use it */
	usedBy: string[];
	/** core = most users need it, recommended = common workflows, optional = niche */
	tier: DepTier;
	/** Check if the dep is available */
	check: () => boolean;
	/** Shell command(s) to install, in preference order per platform */
	install: InstallOption[];
	/** URL for manual install instructions */
	url?: string;
	/** Dep IDs that must be installed first */
	requires?: string[];
}

export interface InstallOption {
	/** Platform: "darwin", "linux", or "any" */
	platform: "darwin" | "linux" | "any";
	/** Shell command */
	cmd: string;
}

function hasCmd(cmd: string): boolean {
	try {
		execSync(`which ${cmd}`, { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

/**
 * Detect immutable/atomic Linux distros (Bazzite, Silverblue, Kinoite, etc.)
 * where dnf/apt are unavailable or aliased to guides. These distros typically
 * use Homebrew (Linuxbrew) or Flatpak for user-space packages.
 */
function isImmutableLinux(): boolean {
	if (process.platform !== "linux") return false;
	try {
		const osRelease = execSync("cat /etc/os-release 2>/dev/null", { encoding: "utf-8" });
		// Bazzite, Silverblue, Kinoite, Aurora, Bluefin — all Fedora Atomic variants
		return /VARIANT_ID=.*(silverblue|kinoite|bazzite|aurora|bluefin|atomic)/i.test(osRelease)
			|| /ostree/i.test(osRelease);
	} catch {
		return false;
	}
}

/** Cached immutable Linux detection */
const _isImmutable = isImmutableLinux();

/** Get the best install command for the current platform */
export function bestInstallCmd(dep: Dep): string | undefined {
	const plat = process.platform === "darwin" ? "darwin" : "linux";

	// On immutable Linux (Bazzite, Silverblue, etc.), dnf/apt are unavailable
	// or aliased to documentation guides. Prefer brew commands.
	// On regular Linux, prefer non-brew (apt/dnf) unless brew is the only option.
	const hasBrew = hasCmd("brew");
	if (plat === "linux" && (_isImmutable || !hasBrew)) {
		// Immutable: must use brew (skip apt/dnf). Regular without brew: skip brew commands.
		const candidates = dep.install.filter((o) => o.platform === plat || o.platform === "any");
		if (_isImmutable && hasBrew) {
			const brewCmd = candidates.find((o) => o.cmd.startsWith("brew "));
			if (brewCmd) return brewCmd.cmd;
		} else if (!_isImmutable) {
			const nonBrew = candidates.find((o) => !o.cmd.startsWith("brew "));
			if (nonBrew) return nonBrew.cmd;
		}
	}

	return (
		dep.install.find((o) => o.platform === plat)?.cmd ??
		dep.install.find((o) => o.platform === "any")?.cmd ??
		dep.install[0]?.cmd
	);
}

/** Get all install options formatted for display */
export function installHints(dep: Dep): string[] {
	return dep.install.map((o) =>
		o.platform === "any" ? o.cmd : `${o.cmd}  (${o.platform})`,
	);
}

/**
 * The canonical dependency registry.
 *
 * Extensions should NOT duplicate these checks — import from here.
 * Order matters: displayed in this order during bootstrap.
 */
export const DEPS: Dep[] = [
	// --- Core: most users want these ---
	{
		id: "ollama",
		name: "Ollama",
		purpose: "Local model inference, embeddings for semantic memory search",
		usedBy: ["local-inference", "project-memory", "cleave", "offline-driver"],
		tier: "core",
		check: () => hasCmd("ollama"),
		install: [
			{ platform: "darwin", cmd: "brew install ollama" },
			{ platform: "linux", cmd: "curl -fsSL https://ollama.com/install.sh | sh" },
		],
		url: "https://ollama.com",
	},
	{
		id: "d2",
		name: "D2",
		purpose: "Diagram rendering (architecture, flowcharts, ER diagrams)",
		usedBy: ["render", "view"],
		tier: "core",
		check: () => hasCmd("d2"),
		install: [
			{ platform: "darwin", cmd: "brew install d2" },
			{ platform: "linux", cmd: "curl -fsSL https://d2lang.com/install.sh | sh" },
		],
		url: "https://d2lang.com",
	},

	// --- Recommended: common workflows ---
	{
		id: "gh",
		name: "GitHub CLI",
		purpose: "GitHub authentication, PR creation, issue management",
		usedBy: ["01-auth"],
		tier: "recommended",
		check: () => hasCmd("gh"),
		install: [
			{ platform: "darwin", cmd: "brew install gh" },
			{ platform: "linux", cmd: "brew install gh" },
			{ platform: "linux", cmd: "sudo apt install gh || sudo dnf install gh" },
		],
		url: "https://cli.github.com",
	},
	{
		id: "pandoc",
		name: "Pandoc",
		purpose: "Document conversion (DOCX, PPTX, EPUB → Markdown)",
		usedBy: ["view"],
		tier: "recommended",
		check: () => hasCmd("pandoc"),
		install: [
			{ platform: "darwin", cmd: "brew install pandoc" },
			{ platform: "linux", cmd: "sudo apt install pandoc || sudo dnf install pandoc" },
		],
		url: "https://pandoc.org",
	},
	{
		id: "cargo",
		name: "Rust toolchain",
		purpose: "Required to build mdserve from source",
		usedBy: ["vault (build dep)"],
		tier: "recommended",
		check: () => hasCmd("cargo"),
		install: [
			// -s -- -y passes -y to rustup-init, suppressing the interactive
			// "1) Proceed / 2) Customise / 3) Cancel" prompt that otherwise hangs.
			{ platform: "any", cmd: "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y" },
		],
		url: "https://rustup.rs",
	},
	{
		id: "mdserve",
		name: "mdserve",
		purpose: "Markdown viewport with wikilinks and graph view (/vault)",
		usedBy: ["vault"],
		tier: "recommended",
		check: () => hasCmd("mdserve"),
		requires: ["cargo"],
		install: [
			{ platform: "any", cmd: "cargo install --git https://github.com/cwilson613/mdserve --branch feature/wikilinks-graph" },
		],
		url: "https://github.com/cwilson613/mdserve",
	},

	// --- Optional: niche or platform-specific ---
	{
		id: "rsvg-convert",
		name: "librsvg",
		purpose: "SVG rendering in terminal",
		usedBy: ["view"],
		tier: "optional",
		check: () => hasCmd("rsvg-convert"),
		install: [
			{ platform: "darwin", cmd: "brew install librsvg" },
			{ platform: "linux", cmd: "brew install librsvg" },
			{ platform: "linux", cmd: "sudo apt install librsvg2-bin" },
		],
	},
	{
		id: "pdftoppm",
		name: "Poppler",
		purpose: "PDF rendering in terminal",
		usedBy: ["view"],
		tier: "optional",
		check: () => hasCmd("pdftoppm"),
		install: [
			{ platform: "darwin", cmd: "brew install poppler" },
			{ platform: "linux", cmd: "brew install poppler" },
			{ platform: "linux", cmd: "sudo apt install poppler-utils" },
		],
	},
	{
		id: "uv",
		name: "uv",
		purpose: "Python package manager for mflux (local image generation)",
		usedBy: ["render"],
		tier: "optional",
		check: () => hasCmd("uv"),
		install: [
			{ platform: "darwin", cmd: "brew install uv" },
			{ platform: "any", cmd: "curl -LsSf https://astral.sh/uv/install.sh | sh" },
		],
		url: "https://docs.astral.sh/uv/",
	},
	{
		id: "aws",
		name: "AWS CLI",
		purpose: "AWS authentication and ECR access",
		usedBy: ["01-auth"],
		tier: "optional",
		check: () => hasCmd("aws"),
		install: [
			{ platform: "darwin", cmd: "brew install awscli" },
			{ platform: "linux", cmd: "brew install awscli" },
			{ platform: "linux", cmd: "sudo apt install awscli" },
		],
	},
	{
		id: "kubectl",
		name: "kubectl",
		purpose: "Kubernetes cluster access",
		usedBy: ["01-auth"],
		tier: "optional",
		check: () => hasCmd("kubectl"),
		install: [
			{ platform: "darwin", cmd: "brew install kubectl" },
			{ platform: "linux", cmd: "brew install kubectl" },
			{ platform: "linux", cmd: "sudo apt install kubectl" },
		],
	},
];

export type DepStatus = { dep: Dep; available: boolean };

/** Check all deps and return their status */
export function checkAll(): DepStatus[] {
	return DEPS.map((dep) => ({
		dep,
		available: dep.check(),
	}));
}

/**
 * Detect whether the terminal supports Unicode emoji rendering.
 *
 * Returns true for modern terminals (Windows Terminal, VS Code, xterm-256color,
 * iTerm2, etc.) and false for legacy consoles (Windows conhost.exe) where emoji
 * render as blank boxes.  Errs on the side of ASCII when uncertain.
 */
function supportsEmoji(): boolean {
	// Windows Terminal sets WT_SESSION; conhost.exe does not
	if (process.env["WT_SESSION"]) return true;
	// VS Code integrated terminal
	if (process.env["TERM_PROGRAM"] === "vscode") return true;
	// iTerm2, Hyper, and other macOS/Linux terminals advertising 256-color
	if (process.env["TERM_PROGRAM"] === "iTerm.app") return true;
	// xterm-256color and similar modern TERM values
	const term = process.env["TERM"] ?? "";
	if (term.includes("256color") || term === "xterm-kitty") return true;
	// COLORTERM=truecolor or 24bit signals a modern terminal
	const colorterm = process.env["COLORTERM"] ?? "";
	if (colorterm === "truecolor" || colorterm === "24bit") return true;
	// CI environments typically render emoji correctly
	if (process.env["CI"]) return true;
	// Non-Windows: default to emoji; on Windows without the above signals, use ASCII
	return process.platform !== "win32";
}

/** Format a single dep status as a line, with install hint if missing */
function formatStatus(s: DepStatus): string {
	const emoji = supportsEmoji();
	const icon = s.available ? (emoji ? "✅" : "[ok]") : (emoji ? "❌" : "[x]");
	let line = `${icon}  ${s.dep.name} — ${s.dep.purpose}`;
	if (!s.available) {
		const cmd = bestInstallCmd(s.dep);
		if (cmd) line += `\n      -> \`${cmd}\``;
	}
	return line;
}

/** Format full report grouped by tier */
export function formatReport(statuses: DepStatus[]): string {
	const tiers: DepTier[] = ["core", "recommended", "optional"];
	const tierLabels: Record<DepTier, string> = {
		core: "Core (most users need these)",
		recommended: "Recommended (common workflows)",
		optional: "Optional (niche / platform-specific)",
	};

	const lines: string[] = ["# Omegon Dependencies\n"];

	for (const tier of tiers) {
		const group = statuses.filter((s) => s.dep.tier === tier);
		if (group.length === 0) continue;

		lines.push(`## ${tierLabels[tier]}\n`);
		for (const s of group) {
			lines.push(formatStatus(s));
		}
		lines.push("");
	}

	const missing = statuses.filter((s) => !s.available);
	const emoji = supportsEmoji();
	if (missing.length === 0) {
		lines.push(emoji ? "🎉 All dependencies are available!" : "[ok] All dependencies are available!");
	} else {
		lines.push(`${emoji ? "⚠️ " : "[!] "}**${missing.length} missing** — run \`/bootstrap\` to install interactively.`);
	}

	return lines.join("\n");
}

/** Topological sort: deps with `requires` come after their prerequisites */
export function sortByRequires(deps: DepStatus[]): DepStatus[] {
	const byId = new Map(deps.map((s) => [s.dep.id, s]));
	const sorted: DepStatus[] = [];
	const visited = new Set<string>();

	function visit(s: DepStatus) {
		if (visited.has(s.dep.id)) return;
		visited.add(s.dep.id);
		for (const reqId of s.dep.requires ?? []) {
			const req = byId.get(reqId);
			if (req && !req.available) visit(req);
		}
		sorted.push(s);
	}

	for (const s of deps) visit(s);
	return sorted;
}
