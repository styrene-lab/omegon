/**
 * Native cleave dispatch — calls the Rust omegon-agent cleave subcommand.
 *
 * This replaces the TypeScript dispatcher (dispatchChildren + spawnChildRpc/Pipe)
 * with a single Rust binary invocation that handles:
 *   - Child spawning (each child is an omegon-agent --prompt)
 *   - Dependency wave ordering
 *   - Idle timeout + wall-clock timeout
 *   - State persistence to state.json
 *   - Worktree cleanup + branch merge
 *
 * The TS caller is responsible for:
 *   - OpenSpec enrichment of task files (before calling this)
 *   - Dashboard state emission (via onProgress)
 *   - Post-merge reporting and lifecycle reconciliation
 */

import { spawn } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { resolveNativeAgent } from "../lib/omegon-subprocess.ts";
import type { CleaveState, ChildState } from "./types.ts";

export interface NativeDispatchConfig {
	/** Path to the plan JSON (written to disk by caller). */
	planPath: string;
	/** The directive text. */
	directive: string;
	/** Workspace directory (worktrees + state.json). */
	workspacePath: string;
	/** Repo root. */
	repoPath: string;
	/** Model identifier (provider:model). */
	model: string;
	/** Max parallel children. */
	maxParallel: number;
	/** Per-child wall-clock timeout in seconds. */
	timeoutSecs: number;
	/** Per-child idle timeout in seconds. */
	idleTimeoutSecs: number;
	/** Max turns per child. */
	maxTurns: number;
}

export interface NativeDispatchResult {
	exitCode: number;
	/** Parsed state.json after completion. */
	state: any;
	/** Raw stderr output. */
	stderr: string;
}

/**
 * Dispatch children via the Rust omegon-agent cleave binary.
 *
 * Returns when the binary exits. The caller should read state.json
 * from the workspace for detailed per-child results.
 */
export async function dispatchViaNative(
	config: NativeDispatchConfig,
	signal?: AbortSignal,
	onProgress?: (line: string) => void,
): Promise<NativeDispatchResult> {
	const nativeAgent = resolveNativeAgent();
	if (!nativeAgent) {
		throw new Error(
			"Native agent binary not found. Run `cargo build --release` in core/. " +
			"TypeScript child dispatch has been removed.",
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

	return new Promise<NativeDispatchResult>((resolve, reject) => {
		const proc = spawn(nativeAgent.binaryPath, args, {
			cwd: config.repoPath,
			stdio: ["ignore", "pipe", "pipe"],
			env: {
				...process.env,
				RUST_LOG: "info",
			},
		});

		let stderr = "";

		proc.stderr?.on("data", (data) => {
			const text = data.toString();
			stderr += text;
			// Forward progress lines
			for (const line of text.split("\n")) {
				const trimmed = line.trim();
				if (trimmed) {
					onProgress?.(trimmed);
				}
			}
		});

		let stdout = "";
		proc.stdout?.on("data", (data) => {
			stdout += data.toString();
		});

		if (signal) {
			const onAbort = () => {
				try { proc.kill("SIGTERM"); } catch { /* already dead */ }
				setTimeout(() => {
					try { proc.kill("SIGKILL"); } catch { /* already dead */ }
				}, 3000);
			};
			signal.addEventListener("abort", onAbort, { once: true });
			proc.on("close", () => signal.removeEventListener("abort", onAbort));
		}

		proc.on("error", (err) => {
			reject(new Error(`Failed to spawn omegon-agent cleave: ${err.message}`));
		});

		proc.on("close", (code) => {
			// Read state.json for detailed results
			let state: any = null;
			try {
				const statePath = join(config.workspacePath, "state.json");
				const raw = readFileSync(statePath, "utf-8");
				state = JSON.parse(raw);
				onProgress?.(`[native-dispatch] exitCode=${code}, children=${state?.children?.length}, statuses=${state?.children?.map((c: any) => c.status).join(",")}`);
			} catch (e: any) {
				onProgress?.(`[native-dispatch] exitCode=${code}, state.json read failed: ${e.message}`);
			}

			resolve({
				exitCode: code ?? 1,
				state,
				stderr,
			});
		});
	});
}
