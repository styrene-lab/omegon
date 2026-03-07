/**
 * vault — Markdown viewport extension
 *
 * Spawns mdserve to render interlinked project markdown as a navigable
 * web UI with wikilink resolution, graph view, and live reload.
 *
 * Auto-starts mdserve on session_start if the binary is on $PATH.
 * Stores port in sharedState for URI resolver consumption.
 *
 * Commands:
 *   /vault           — Show status (running/stopped, port, PID)
 *   /vault [path]    — Start mdserve on a specific directory
 *   /vault stop      — Stop the running mdserve instance
 *   /vault graph     — Open the graph view in the browser
 */

import { execSync, spawn, type ChildProcess } from "node:child_process";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const DEFAULT_PORT = 3333;
const BINARY_NAME = "mdserve";

let mdserveProcess: ChildProcess | null = null;
let mdservePort: number | null = null;
let mdserveDir: string | null = null;

/** Get the current mdserve port, or null if not running. Used by uri-resolver. */
export function getMdservePort(): number | null {
	return mdservePort;
}

function hasBinary(): boolean {
	try {
		execSync(`which ${BINARY_NAME}`, { stdio: "ignore" });
		return true;
	} catch {
		return false;
	}
}

function openBrowser(url: string): void {
	try {
		const cmd = process.platform === "darwin" ? "open" : "xdg-open";
		spawn(cmd, [url], { stdio: "ignore", detached: true }).unref();
	} catch { /* user can open manually */ }
}

// No shared state update needed — getMdservePort() is the public API

function stopMdserve(): string {
	if (mdserveProcess) {
		const pid = mdserveProcess.pid;
		mdserveProcess.kill("SIGTERM");
		mdserveProcess = null;
		const msg = `Stopped mdserve (PID ${pid}, was serving ${mdserveDir} on port ${mdservePort})`;
		mdservePort = null;
		mdserveDir = null;
		
		return msg;
	}
	return "mdserve is not running.";
}

function spawnMdserve(dir: string, port: number, options?: { silent?: boolean }): string {
	if (mdserveProcess) {
		if (mdserveDir === dir) {
			return `mdserve already running at http://127.0.0.1:${mdservePort} (PID ${mdserveProcess.pid})\n` +
				`Serving: ${mdserveDir}\n` +
				`Use \`/vault stop\` to stop, or \`/vault graph\` to open graph view.`;
		}
		stopMdserve();
	}

	const child = spawn(BINARY_NAME, [dir, "--port", String(port)], {
		stdio: ["ignore", "pipe", "pipe"],
		detached: false,
	});

	mdserveProcess = child;
	mdservePort = port;
	mdserveDir = dir;
	

	child.stdout?.on("data", (data: Buffer) => {
		const match = data.toString().match(/using (\d+) instead/);
		if (match) {
			mdservePort = parseInt(match[1], 10);
			
		}
	});

	child.on("exit", () => {
		if (mdserveProcess === child) {
			mdserveProcess = null;
			mdservePort = null;
			mdserveDir = null;
			
		}
	});

	if (!options?.silent) {
		openBrowser(`http://127.0.0.1:${port}`);
	}

	const prefix = options?.silent ? "Auto-started" : "Started";
	return `${prefix} mdserve at http://127.0.0.1:${port} (PID ${child.pid})\n` +
		`Serving: ${dir}\n` +
		`Graph view: http://127.0.0.1:${port}/graph\n` +
		`Use \`/vault stop\` to stop.`;
}

const NOT_INSTALLED = "`mdserve` is not installed. Run `/bootstrap` to set up pi-kit dependencies.";

export default function (pi: ExtensionAPI) {
	

	// Auto-start mdserve on session start if binary is available
	pi.on("session_start", () => {
		if (hasBinary()) {
			spawnMdserve(process.cwd(), DEFAULT_PORT, { silent: true });
		}
	});

	pi.on("session_shutdown", () => {
		if (mdserveProcess) {
			mdserveProcess.kill("SIGTERM");
			mdserveProcess = null;
			mdservePort = null;
			mdserveDir = null;
			
		}
	});

	pi.registerCommand("vault", {
		description: "Markdown viewport — serve project docs with wikilinks and graph view",
		handler: async (args, ctx) => {
			const subcommand = args.trim().split(/\s+/)[0]?.toLowerCase() || "";

			switch (subcommand) {
				case "stop":
					ctx.ui.notify(stopMdserve(), "info");
					return;

				case "status":
				case "": {
					// Default: show status
					if (mdserveProcess) {
						ctx.ui.notify(
							`mdserve is running (PID ${mdserveProcess.pid})\n` +
							`URL: http://127.0.0.1:${mdservePort}\n` +
							`Serving: ${mdserveDir}`,
							"info",
						);
					} else if (!hasBinary()) {
						ctx.ui.notify(NOT_INSTALLED, "warning");
					} else {
						ctx.ui.notify("mdserve is not running. Use `/vault [path]` to start.", "info");
					}
					return;
				}

				case "graph":
					if (!hasBinary()) { ctx.ui.notify(NOT_INSTALLED, "warning"); return; }
					if (mdserveProcess && mdservePort) {
						openBrowser(`http://127.0.0.1:${mdservePort}/graph`);
						ctx.ui.notify(`Opened graph view at http://127.0.0.1:${mdservePort}/graph`, "info");
					} else {
						const dir = process.cwd();
						ctx.ui.notify(spawnMdserve(dir, DEFAULT_PORT), "info");
						setTimeout(() => {
							if (mdservePort) openBrowser(`http://127.0.0.1:${mdservePort}/graph`);
						}, 1000);
					}
					return;

				default: {
					if (!hasBinary()) { ctx.ui.notify(NOT_INSTALLED, "warning"); return; }
					const dir = subcommand;
					ctx.ui.notify(spawnMdserve(dir, DEFAULT_PORT), "info");
					return;
				}
			}
		},
	});
}
