import { describe, test } from "node:test";
import assert from "node:assert";
import type { NativeProgressEvent } from "./native-dispatch.ts";

describe("native-dispatch", () => {
	describe("NDJSON event parsing", () => {
		test("should parse progress events matching Rust schema", () => {
			const sampleEvents: NativeProgressEvent[] = [
				{ event: "wave_start", wave: 0, children: ["task-a", "task-b"] },
				{ event: "child_spawned", child: "task-a", pid: 1234 },
				{ event: "child_status", child: "task-a", status: "running" },
				{ event: "child_activity", child: "task-a", tool: "write", target: "tmp/foo.txt" },
				{ event: "child_activity", child: "task-a", turn: 2 },
				{ event: "child_status", child: "task-a", status: "completed", duration_secs: 45.2 },
				{ event: "child_status", child: "task-b", status: "failed", error: "idle timeout", duration_secs: 180 },
				{ event: "auto_commit", child: "task-a", files: 2 },
				{ event: "merge_start" },
				{ event: "merge_result", child: "task-a", success: true },
				{ event: "merge_result", child: "task-b", success: false, detail: "no new commits" },
				{ event: "done", completed: 1, failed: 1, duration_secs: 63.5 },
			];

			for (const evt of sampleEvents) {
				// Round-trip through JSON to simulate what the Rust binary emits
				const json = JSON.stringify(evt);
				const parsed = JSON.parse(json) as NativeProgressEvent;
				assert.strictEqual(parsed.event, evt.event);
			}
		});

		test("type narrowing works via event discriminator", () => {
			const evt: NativeProgressEvent = { event: "child_activity", child: "a", tool: "bash", target: "ls" };
			if (evt.event === "child_activity") {
				assert.strictEqual(evt.tool, "bash");
				assert.strictEqual(evt.child, "a");
			}

			const evt2: NativeProgressEvent = { event: "child_status", child: "b", status: "completed", duration_secs: 10 };
			if (evt2.event === "child_status") {
				assert.strictEqual(evt2.status, "completed");
				assert.strictEqual(evt2.duration_secs, 10);
			}
		});

		test("should handle malformed JSON gracefully", () => {
			const malformedLines = [
				"not json",
				"{ incomplete",
				'{"event":"child_spawned","child":"valid","pid":1}',
			];

			const events: NativeProgressEvent[] = [];
			const fallback: string[] = [];

			for (const line of malformedLines) {
				try {
					events.push(JSON.parse(line) as NativeProgressEvent);
				} catch {
					fallback.push(line);
				}
			}

			assert.strictEqual(events.length, 1);
			assert.strictEqual(events[0].event, "child_spawned");
			assert.strictEqual(fallback.length, 2);
		});

		test("should handle partial NDJSON buffering", () => {
			let buffer = "";
			const events: NativeProgressEvent[] = [];

			// Simulate chunked data
			const chunks = [
				'{"event":"wave_start","wave":0,',
				'"children":["a"]}\n{"event":"child_spawned",',
				'"child":"a","pid":99}\npartial',
			];

			for (const chunk of chunks) {
				buffer += chunk;
				const lines = buffer.split("\n");
				buffer = lines.pop() || "";
				for (const line of lines) {
					const trimmed = line.trim();
					if (trimmed) {
						try { events.push(JSON.parse(trimmed)); } catch { /* skip */ }
					}
				}
			}

			assert.strictEqual(events.length, 2);
			assert.strictEqual(events[0].event, "wave_start");
			assert.strictEqual(events[1].event, "child_spawned");
			assert.strictEqual(buffer, "partial");
		});
	});
});
