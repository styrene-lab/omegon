/**
 * Native cleave dispatch — calls the Rust omegon-agent cleave subcommand.
 */

import { spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { resolveNativeAgent } from "../lib/omegon-subprocess.ts";

export interface NativeDispatchConfig {
	planPath: string;
	directive: string;
	workspacePath: string;
	repoPath: string;
	model: string;
	maxParallel: number;
	timeoutSecs: number;
	idleTimeoutSecs: number;
	maxTurns: number;
}

/**
 * Native progress events emitted by the Rust omegon-agent cleave subcommand.
 * Discriminated on the `event` field (matches Rust `#[serde(tag = "event")]`).
 */
export type NativeProgressEvent =
	| { event: "wave_start"; wave: number; children: string[] }
	| { event: "child_spawned"; child: string; pid: number }
	| { event: "child_status"; child: string; status: "running" | "completed" | "failed"; duration_secs?: number; error?: string }
	| { event: "child_activity"; child: string; turn?: number; tool?: string; target?: string }
	| { event: "auto_commit"; child: string; files: number }
	| { event: "merge_start" }
	| { event: "merge_result"; child: string; success: boolean; detail?: string }
	| { event: "done"; completed: number; failed: number; duration_secs: number };

/** Shape of the Rust CleaveState.ChildState after serde(rename_all = "camelCase"). */
export interface RustChildState {
	childId: number;
	label: string;
	description: string;
	scope: string[];
	dependsOn: string[];
	status: "pending" | "running" | "completed" | "failed";
	error?: string;
	branch?: string;
	worktreePath?: string;
	backend: string;
	executeModel?: string;
	durationSecs?: number;
}

/** Shape of the Rust CleaveState after serde. */
export interface RustCleaveState {
	runId: string;
	directive: string;
	repoPath: string;
	workspacePath: string;
	children: RustChildState[];
	plan: unknown;
}

export interface NativeDispatchResult {
	exitCode: number;
	state: RustCleaveState | null;
	stderr: string;
}

export async function dispatchViaNative(
	config: NativeDispatchConfig,
	signal?: AbortSignal,
	onProgress?: (line: string) => void,
	onEvent?: (event: NativeProgressEvent) => void,
): Promise<NativeDispatchResult> {
	const nativeAgent = resolveNativeAgent();
	if (!nativeAgent) {
		throw new Error(
			"Native agent binary not found. Run `cargo build --release` in core/.",
		);
	}

	const args = [
		"cleave",
		"--plan", config.planPath,
		"--directive", config.directive,
		"--workspace", config.workspacePath,
		"--cwd", config.repoPath,
		"--model", config.model,
		"--max-parallel", String(config.maxParallel),
		"--timeout", String(config.timeoutSecs),
		"--idle-timeout", String(config.idleTimeoutSecs),
		"--max-turns", String(config.maxTurns),
		"--bridge", nativeAgent.bridgePath,
	];

	const log = (msg: string) => onProgress?.(`[native-dispatch] ${msg}`);

	log(`binary: ${nativeAgent.binaryPath}`);
	log(`bridge: ${nativeAgent.bridgePath}`);
	log(`workspace: ${config.workspacePath}`);
	log(`model: ${config.model}`);
	log(`spawning: ${nativeAgent.binaryPath} ${args.join(" ")}`);

	return new Promise<NativeDispatchResult>((resolve, reject) => {
		let proc: ReturnType<typeof spawn>;
		try {
			proc = spawn(nativeAgent.binaryPath, args, {
				cwd: config.repoPath,
				stdio: ["ignore", "pipe", "pipe"],
				env: {
					...process.env,
					RUST_LOG: "info",
				},
			});
		} catch (e: any) {
			log(`spawn threw: ${e.message}`);
			reject(new Error(`Failed to spawn omegon-agent cleave: ${e.message}`));
			return;
		}

		log(`spawned pid=${proc.pid}`);

		/** Capped stderr buffer — keeps last 64 KB for diagnostics without unbounded growth. */
		const STDERR_CAP = 64 * 1024;
		let stderr = "";
		let stderrLines = 0;

		// Parse stdout as NDJSON progress events
		let stdoutBuffer = "";
		proc.stdout?.on("data", (data: Buffer) => {
			stdoutBuffer += data.toString();
			const lines = stdoutBuffer.split("\n");
			// Keep the last incomplete line in the buffer
			stdoutBuffer = lines.pop() || "";
			
			for (const line of lines) {
				const trimmed = line.trim();
				if (trimmed && onEvent) {
					try {
						const event = JSON.parse(trimmed) as NativeProgressEvent;
						onEvent(event);
					} catch (e) {
						// Non-JSON lines on stdout - treat as progress messages
						onProgress?.(`[stdout] ${trimmed}`);
					}
				}
			}
		});

		proc.stderr?.on("data", (data: Buffer) => {
			const text = data.toString();
			if (stderr.length < STDERR_CAP) {
				stderr += text;
				if (stderr.length > STDERR_CAP) stderr = stderr.slice(-STDERR_CAP);
			}
			for (const line of text.split("\n")) {
				const trimmed = line.trim();
				if (trimmed) {
					stderrLines++;
					onProgress?.(trimmed);
				}
			}
		});

		if (signal) {
			if (signal.aborted) {
				log("signal already aborted before spawn!");
				try { proc.kill("SIGTERM"); } catch { /* */ }
				resolve({ exitCode: 130, state: null, stderr: "aborted before start" });
				return;
			}
			const onAbort = () => {
				log("abort signal received — killing child");
				try { proc.kill("SIGTERM"); } catch { /* */ }
				setTimeout(() => {
					try { proc.kill("SIGKILL"); } catch { /* */ }
				}, 3000);
			};
			signal.addEventListener("abort", onAbort, { once: true });
			proc.on("close", () => signal.removeEventListener("abort", onAbort));
		}

		proc.on("error", (err) => {
			log(`proc error event: ${err.message}`);
			reject(new Error(`Failed to spawn omegon-agent cleave: ${err.message}`));
		});

		proc.on("close", (code, sig) => {
			log(`proc closed: code=${code} signal=${sig} stderrLines=${stderrLines}`);

			let state: RustCleaveState | null = null;
			try {
				const statePath = join(config.workspacePath, "state.json");
				const raw = readFileSync(statePath, "utf-8");
				const parsed = JSON.parse(raw) as RustCleaveState;
				// Validate minimum shape
				if (parsed && Array.isArray(parsed.children)) {
					state = parsed;
					const statuses = state.children.map((c) => `${c.label}=${c.status}`).join(", ");
					log(`state.json loaded: ${state.children.length} children, [${statuses}]`);
				} else {
					log("state.json loaded but missing children array");
				}
			} catch (e: any) {
				log(`state.json read failed: ${e.message}`);
			}

			resolve({
				exitCode: code ?? 1,
				state,
				stderr,
			});
		});
	});
}
