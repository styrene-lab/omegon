/**
 * Tests for cleave/rpc-child — RPC child communication module.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import {
	sendRpcCommand,
	buildPromptCommand,
	buildAbortCommand,
	parseRpcEventStream,
	mapEventToProgress,
} from "./rpc-child.ts";
import type { RpcChildEvent } from "./types.ts";

// ─── JSON Framing ───────────────────────────────────────────────────────────

describe("sendRpcCommand", () => {
	it("writes a JSON line with LF terminator to stdin", () => {
		const chunks: string[] = [];
		const stdin = new PassThrough();
		stdin.on("data", (d) => chunks.push(d.toString()));

		const ok = sendRpcCommand(stdin, { type: "prompt", message: "hello" });
		assert.equal(ok, true);

		const written = chunks.join("");
		assert.equal(written, '{"type":"prompt","message":"hello"}\n');
	});

	it("returns false when stdin is not writable", () => {
		const stdin = new PassThrough();
		stdin.end(); // close it
		// After end(), writable becomes false eventually; force it
		(stdin as any).writable = false;

		const ok = sendRpcCommand(stdin, { type: "abort" });
		assert.equal(ok, false);
	});
});

describe("buildPromptCommand", () => {
	it("builds a prompt command with message", () => {
		const cmd = buildPromptCommand("do the thing");
		assert.deepEqual(cmd, { type: "prompt", message: "do the thing" });
	});

	it("includes id when provided", () => {
		const cmd = buildPromptCommand("do it", "req_1");
		assert.deepEqual(cmd, { type: "prompt", message: "do it", id: "req_1" });
	});
});

describe("buildAbortCommand", () => {
	it("builds an abort command", () => {
		assert.deepEqual(buildAbortCommand(), { type: "abort" });
	});

	it("includes id when provided", () => {
		assert.deepEqual(buildAbortCommand("req_2"), { type: "abort", id: "req_2" });
	});
});

// ─── Event Stream Parser ────────────────────────────────────────────────────

describe("parseRpcEventStream", () => {
	it("parses JSON lines into typed events", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		stdout.write('{"type":"agent_start"}\n');
		stdout.write('{"type":"tool_execution_start","toolCallId":"tc1","toolName":"read","args":{"path":"foo.ts"}}\n');
		stdout.write('{"type":"agent_end","messages":[]}\n');
		stdout.end();

		await collecting;

		// Last event is always pipe_closed
		assert.equal(events.length, 4);
		assert.equal(events[0].type, "agent_start");
		assert.equal(events[1].type, "tool_execution_start");
		if (events[1].type === "tool_execution_start") {
			assert.equal(events[1].toolName, "read");
		}
		assert.equal(events[2].type, "agent_end");
		assert.equal(events[3].type, "pipe_closed");
	});

	it("skips non-JSON lines silently", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		stdout.write("some debug output\n");
		stdout.write('{"type":"agent_start"}\n');
		stdout.write("another debug line\n");
		stdout.write('{"invalid json\n');
		stdout.end();

		await collecting;

		assert.equal(events.length, 2); // agent_start + pipe_closed
		assert.equal(events[0].type, "agent_start");
		assert.equal(events[1].type, "pipe_closed");
	});

	it("handles lines without type field", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		stdout.write('{"foo":"bar"}\n'); // valid JSON but no type
		stdout.write('{"type":"agent_start"}\n');
		stdout.end();

		await collecting;

		assert.equal(events.length, 2); // agent_start + pipe_closed
		assert.equal(events[0].type, "agent_start");
	});

	it("emits pipe_closed on stream end (graceful degradation)", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		// Close immediately — simulates pipe break
		stdout.end();

		await collecting;

		assert.equal(events.length, 1);
		assert.equal(events[0].type, "pipe_closed");
	});

	it("emits pipe_closed on stream error (pipe break)", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		stdout.write('{"type":"agent_start"}\n');
		// Give time for the data event to process
		await new Promise((r) => setTimeout(r, 10));
		stdout.destroy(new Error("pipe broken"));

		await collecting;

		assert.equal(events[0].type, "agent_start");
		// Should end with pipe_closed
		assert.equal(events[events.length - 1].type, "pipe_closed");
	});

	it("handles chunked delivery across JSON boundaries", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		// Send partial JSON across chunks
		stdout.write('{"type":"agen');
		stdout.write('t_start"}\n{"type":');
		stdout.write('"agent_end"}\n');
		stdout.end();

		await collecting;

		assert.equal(events.length, 3); // agent_start + agent_end + pipe_closed
		assert.equal(events[0].type, "agent_start");
		assert.equal(events[1].type, "agent_end");
	});

	it("handles CRLF line endings", async () => {
		const stdout = new PassThrough();
		const events: RpcChildEvent[] = [];

		const iter = parseRpcEventStream(stdout);
		const collecting = (async () => {
			for await (const event of iter) {
				events.push(event);
			}
		})();

		stdout.write('{"type":"agent_start"}\r\n');
		stdout.end();

		await collecting;

		assert.equal(events[0].type, "agent_start");
	});
});

// ─── Event-to-Progress Mapping ──────────────────────────────────────────────

describe("mapEventToProgress", () => {
	it("maps tool_execution_start to tool progress with path", () => {
		const result = mapEventToProgress({
			type: "tool_execution_start",
			toolCallId: "tc1",
			toolName: "read",
			args: { path: "src/auth.ts" },
		});
		assert.deepEqual(result, {
			kind: "tool",
			summary: "tool: read src/auth.ts",
			toolName: "read",
		});
	});

	it("maps tool_execution_start for bash with truncated command", () => {
		const longCmd = "find . -name '*.ts' -exec grep -l 'something very specific and long that exceeds the limit' {} +";
		const result = mapEventToProgress({
			type: "tool_execution_start",
			toolCallId: "tc1",
			toolName: "bash",
			args: { command: longCmd },
		});
		assert.equal(result?.kind, "tool");
		assert.ok(result!.summary.length <= 70); // "tool: bash " + 60 chars max
		assert.ok(result!.summary.endsWith("…"));
	});

	it("maps tool_execution_end with success indicator", () => {
		const result = mapEventToProgress({
			type: "tool_execution_end",
			toolCallId: "tc1",
			toolName: "edit",
			result: {},
			isError: false,
		});
		assert.deepEqual(result, { kind: "tool", summary: "tool: edit ✓", toolName: "edit" });
	});

	it("maps tool_execution_end with error indicator", () => {
		const result = mapEventToProgress({
			type: "tool_execution_end",
			toolCallId: "tc1",
			toolName: "bash",
			result: {},
			isError: true,
		});
		assert.deepEqual(result, { kind: "tool", summary: "tool: bash ✗", toolName: "bash" });
	});

	it("maps agent_start to lifecycle", () => {
		const result = mapEventToProgress({ type: "agent_start" });
		assert.deepEqual(result, { kind: "lifecycle", summary: "Agent started" });
	});

	it("maps agent_end to lifecycle", () => {
		const result = mapEventToProgress({ type: "agent_end" });
		assert.deepEqual(result, { kind: "lifecycle", summary: "Agent completed" });
	});

	it("maps pipe_closed to error", () => {
		const result = mapEventToProgress({ type: "pipe_closed" });
		assert.deepEqual(result, { kind: "error", summary: "Pipe closed" });
	});

	it("returns null for turn_start (no meaningful progress)", () => {
		assert.equal(mapEventToProgress({ type: "turn_start" }), null);
	});

	it("returns null for message_update (too noisy)", () => {
		assert.equal(mapEventToProgress({ type: "message_update" }), null);
	});

	it("returns null for response events", () => {
		assert.equal(
			mapEventToProgress({ type: "response", command: "prompt", success: true }),
			null,
		);
	});

	it("maps auto_retry_start to lifecycle", () => {
		const result = mapEventToProgress({
			type: "auto_retry_start",
			attempt: 2,
			maxAttempts: 3,
			delayMs: 1000,
			errorMessage: "rate limit",
		});
		assert.deepEqual(result, { kind: "lifecycle", summary: "Retry 2/3" });
	});

	it("maps auto_compaction_start to lifecycle", () => {
		const result = mapEventToProgress({
			type: "auto_compaction_start",
			reason: "threshold",
		});
		assert.deepEqual(result, { kind: "lifecycle", summary: "Compacting context…" });
	});
});
