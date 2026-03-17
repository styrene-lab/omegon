/**
 * cleave/rpc-child — RPC child communication module.
 *
 * Provides JSON line framing for stdin commands, stdout event stream parsing,
 * event-to-progress mapping, and pipe-break handling for cleave child processes
 * running in `--mode rpc`.
 *
 * This module is a building block for the dispatcher to use when spawning
 * children in RPC mode instead of pipe mode.
 */

import type { Writable, Readable } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import type { RpcChildEvent, RpcProgressUpdate } from "./types.ts";

// ─── JSON Line Framing (stdin commands) ─────────────────────────────────────

/**
 * Serialize and write a JSON command to a child's stdin.
 * Uses strict LF-only JSONL framing (matching pi-mono's serializeJsonLine).
 *
 * @returns true if the write succeeded, false if stdin is not writable
 */
export function sendRpcCommand(
	stdin: Writable,
	command: Record<string, unknown>,
): boolean {
	if (!stdin.writable) return false;
	try {
		stdin.write(`${JSON.stringify(command)}\n`);
		return true;
	} catch {
		return false;
	}
}

/**
 * Build a prompt command for a cleave child task.
 */
export function buildPromptCommand(
	message: string,
	id?: string,
): Record<string, unknown> {
	const cmd: Record<string, unknown> = { type: "prompt", message };
	if (id !== undefined) cmd.id = id;
	return cmd;
}

/**
 * Build an abort command.
 */
export function buildAbortCommand(id?: string): Record<string, unknown> {
	const cmd: Record<string, unknown> = { type: "abort" };
	if (id !== undefined) cmd.id = id;
	return cmd;
}

// ─── Stdout Event Stream Parser ─────────────────────────────────────────────

/**
 * Parse an RPC event stream from a child's stdout as an async iterator.
 *
 * Yields typed RpcChildEvent objects for each valid JSON line received.
 * Non-JSON lines are silently skipped (child may emit debug output to stdout).
 *
 * When stdout closes (end event), the iterator emits a synthetic
 * `{ type: "pipe_closed" }` event and completes — it does NOT throw.
 * This enables graceful degradation: the caller decides how to handle
 * pipe breaks vs normal completion.
 */
export async function* parseRpcEventStream(
	stdout: Readable,
): AsyncGenerator<RpcChildEvent, void, undefined> {
	// We implement a manual async iteration over the stream data,
	// using LF-only splitting (matching pi-mono's jsonl.ts approach).
	const decoder = new StringDecoder("utf8");
	let buffer = "";
	let done = false;

	// Queue for parsed events, with a resolver for the consumer
	const queue: RpcChildEvent[] = [];
	let waitResolve: (() => void) | null = null;

	function enqueue(event: RpcChildEvent) {
		queue.push(event);
		if (waitResolve) {
			const r = waitResolve;
			waitResolve = null;
			r();
		}
	}

	function processBuffer() {
		while (true) {
			const idx = buffer.indexOf("\n");
			if (idx === -1) break;
			const line = buffer.slice(0, idx);
			buffer = buffer.slice(idx + 1);
			// Strip optional CR
			const clean = line.endsWith("\r") ? line.slice(0, -1) : line;
			if (clean.length === 0) continue;
			try {
				const parsed = JSON.parse(clean);
				if (parsed && typeof parsed === "object" && typeof parsed.type === "string") {
					enqueue(parsed as RpcChildEvent);
				}
			} catch {
				// Non-JSON line — skip silently
			}
		}
	}

	const onData = (chunk: Buffer | string) => {
		buffer += typeof chunk === "string" ? chunk : decoder.write(chunk);
		processBuffer();
	};

	const onEnd = () => {
		buffer += decoder.end();
		processBuffer();
		// Emit synthetic pipe_closed event
		enqueue({ type: "pipe_closed" });
		done = true;
		// Wake up consumer if waiting
		if (waitResolve) {
			const r = waitResolve;
			waitResolve = null;
			r();
		}
	};

	const onError = (_err: Error) => {
		enqueue({ type: "pipe_closed" });
		done = true;
		if (waitResolve) {
			const r = waitResolve;
			waitResolve = null;
			r();
		}
	};

	stdout.on("data", onData);
	stdout.on("end", onEnd);
	stdout.on("error", onError);

	try {
		while (true) {
			if (queue.length > 0) {
				const event = queue.shift()!;
				yield event;
				if (event.type === "pipe_closed") return;
			} else if (done) {
				return;
			} else {
				// Wait for next event
				await new Promise<void>((resolve) => {
					waitResolve = resolve;
				});
			}
		}
	} finally {
		stdout.off("data", onData);
		stdout.off("end", onEnd);
		stdout.off("error", onError);
	}
}

// ─── Event-to-Progress Mapping ──────────────────────────────────────────────

/**
 * Map an RpcChildEvent to a structured progress update for the dashboard.
 *
 * Returns null for events that don't produce meaningful progress (e.g. turn_start).
 */
export function mapEventToProgress(event: RpcChildEvent): RpcProgressUpdate | null {
	switch (event.type) {
		case "agent_start":
			return { kind: "lifecycle", summary: "Agent started" };

		case "agent_end":
			return { kind: "lifecycle", summary: "Agent completed" };

		case "turn_start":
			return null; // No meaningful progress

		case "turn_end":
			return { kind: "lifecycle", summary: "Turn completed" };

		case "message_start":
			return null; // Wait for content

		case "message_update":
			return null; // Too noisy for dashboard

		case "message_end":
			return { kind: "lifecycle", summary: "Message completed" };

		case "tool_execution_start":
			return {
				kind: "tool",
				summary: `tool: ${event.toolName}${formatToolArgs(event.toolName, event.args)}`,
				toolName: event.toolName,
			};

		case "tool_execution_update":
			return null; // Partial results too noisy

		case "tool_execution_end":
			return {
				kind: "tool",
				summary: `tool: ${event.toolName} ${event.isError ? "✗" : "✓"}`,
				toolName: event.toolName,
			};

		case "auto_compaction_start":
			return { kind: "lifecycle", summary: "Compacting context…" };

		case "auto_compaction_end":
			return { kind: "lifecycle", summary: event.aborted ? "Compaction aborted" : "Compaction done" };

		case "auto_retry_start":
			return { kind: "lifecycle", summary: `Retry ${event.attempt}/${event.maxAttempts}` };

		case "auto_retry_end":
			return { kind: "lifecycle", summary: event.success ? "Retry succeeded" : "Retry failed" };

		case "response":
			// RPC response to our command — not progress-relevant
			return null;

		case "pipe_closed":
			return { kind: "error", summary: "Pipe closed" };

		default:
			return null;
	}
}

/**
 * Format tool arguments for display in a concise summary line.
 */
function formatToolArgs(toolName: string, args: unknown): string {
	if (!args || typeof args !== "object") return "";
	const a = args as Record<string, unknown>;
	switch (toolName) {
		case "read":
		case "write":
		case "view":
			return a.path ? ` ${a.path}` : "";
		case "edit":
			return a.path ? ` ${a.path}` : "";
		case "bash":
			if (typeof a.command === "string") {
				const cmd = a.command.length > 60 ? a.command.slice(0, 57) + "…" : a.command;
				return ` ${cmd}`;
			}
			return "";
		default:
			return "";
	}
}
