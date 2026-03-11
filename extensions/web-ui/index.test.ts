import assert from "node:assert/strict";
import { before, beforeEach, afterEach, describe, it } from "node:test";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { startWebUIServer, type WebUIServer } from "./server.ts";
import { _setServer, _setSpawnFn, _getServer } from "./index.ts";
import { buildControlPlaneState } from "./state.ts";
import { sharedState } from "../shared-state.ts";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "../..");
const STARTED_AT = Date.now() - 1000;

function buildFakePi() {
  const commands = new Map<string, { handler: (args: string, ctx: any) => Promise<void> }>();
  const events = new Map<string, Array<() => Promise<void>>>();
  return {
    registerCommand(name: string, config: { handler: (args: string, ctx: any) => Promise<void> }) {
      commands.set(name, config);
    },
    on(event: string, handler: () => Promise<void>) {
      const list = events.get(event) ?? [];
      list.push(handler);
      events.set(event, list);
    },
    _commands: commands,
    async _trigger(event: string) {
      for (const handler of events.get(event) ?? []) await handler();
    },
  };
}

async function runCommand(api: ReturnType<typeof buildFakePi>, args: string): Promise<string[]> {
  const command = api._commands.get("web-ui");
  assert.ok(command, "web-ui command should be registered");
  const messages: string[] = [];
  await command.handler(args, { cwd: process.cwd(), ui: { notify: (msg: string) => messages.push(msg) } });
  return messages;
}

let register: (pi: ReturnType<typeof buildFakePi>) => void;

before(async () => {
  const mod = await import("./index.ts");
  register = mod.default as unknown as typeof register;
});

describe("web-ui command surface", () => {
  let api: ReturnType<typeof buildFakePi>;
  let realServer: WebUIServer | null = null;

  beforeEach(() => {
    _setServer(null);
    api = buildFakePi();
    register(api as any);
  });

  afterEach(async () => {
    if (realServer) {
      await realServer.stop().catch(() => {});
      realServer = null;
    }
    _setServer(null);
  });

  it("reports stopped status before start", async () => {
    const messages = await runCommand(api, "status");
    assert.equal(messages.length, 1);
    assert.match(messages[0], /stopped/i);
  });

  it("starts server and reports URL", async () => {
    const messages = await runCommand(api, "start");
    realServer = _getServer(); // capture before any assertion can throw — ensures afterEach stops the server
    assert.equal(messages.length, 1);
    assert.match(messages[0], /started/i);
    assert.match(messages[0], /127\.0\.0\.1/);
    assert.ok(realServer);
  });

  it("opens browser using explicit argv (no shell string)", async () => {
    realServer = await startWebUIServer();
    _setServer(realServer);

    let capturedCmd: string | null = null;
    let capturedArgs: string[] | null = null;

    const prev = _setSpawnFn(((cmd: string, args: string[], _opts: unknown) => {
      capturedCmd = cmd;
      capturedArgs = args;
      return { on: () => {}, unref: () => {} } as any;
    }) as any);

    try {
      const messages = await runCommand(api, "open");
      assert.equal(messages.length, 1);
      assert.match(messages[0], /Opening/);

      // Must have called spawn with an explicit program
      assert.notEqual(capturedCmd, null, "spawn should have been called");
      assert.notEqual(capturedArgs, null, "spawn should have been called with args");

      // URL must appear as a discrete argument, not interpolated into a shell string
      const url = realServer!.url;
      assert.ok(
        capturedArgs!.includes(url),
        `URL "${url}" should be a discrete spawn argument, got: ${JSON.stringify(capturedArgs)}`
      );

      // The command itself must be a launcher binary, not a shell-formatted string
      const launcher = capturedCmd!;
      assert.ok(
        ["open", "xdg-open", "cmd"].includes(launcher),
        `Expected platform launcher binary, got: "${launcher}"`
      );

      // The shell-string anti-pattern: the command must NOT contain the URL baked in
      assert.ok(
        !launcher.includes(url),
        "Launcher binary must not contain the URL (shell-string anti-pattern)"
      );

      // Platform-specific argv validation:
      // On Windows, cmd.exe `start` requires an empty-string window-title placeholder
      // before the URL so it treats the URL as the target rather than the title.
      if (launcher === "cmd") {
        assert.deepEqual(
          capturedArgs,
          ["/c", "start", "", url],
          `Windows argv must be ["/c","start","",url] — removing "" breaks cmd.exe start semantics`
        );
      } else if (launcher === "open") {
        assert.deepEqual(capturedArgs, [url], `macOS argv must be ["<url>"]`);
      } else {
        assert.deepEqual(capturedArgs, [url], `Linux argv must be ["<url>"]`);
      }
    } finally {
      _setSpawnFn(prev);
    }
  });

  it("stops server gracefully on session shutdown", async () => {
    realServer = await startWebUIServer();
    _setServer(realServer);
    await api._trigger("session_shutdown");
    // Use the explicit getter rather than the ESM live-binding export so this
    // test is not sensitive to Node version differences in live-binding semantics.
    assert.equal(_getServer(), null);
    realServer = null;
  });
});

// ── Metadata parity regression tests ─────────────────────────────────────────
//
// These tests verify that the dashboard snapshot exposes structured operator
// metadata and recovery actionability so web UI consumers never need to
// reverse-engineer those values from footer text or unrelated fields.

describe("dashboard snapshot — operator metadata parity", () => {
  beforeEach(() => {
    sharedState.memoryTokenEstimate = 0;
    sharedState.lastMemoryInjection = undefined;
    sharedState.effort = undefined;
    sharedState.recovery = undefined;
  });

  it("operatorMetadata is always present in the dashboard snapshot", () => {
    const snap = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.ok(
      "operatorMetadata" in snap.dashboard,
      "dashboard snapshot must include operatorMetadata"
    );
    const m = snap.dashboard.operatorMetadata;
    assert.ok(m !== null && typeof m === "object");
  });

  it("operatorMetadata returns nulls when effort extension is inactive", () => {
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    const m = dashboard.operatorMetadata;
    assert.equal(m.effortName, null);
    assert.equal(m.effortLevel, null);
    assert.equal(m.driverTier, null);
    assert.equal(m.thinkingLevel, null);
    assert.equal(m.effortCapped, false);
  });

  it("operatorMetadata reflects effort tier state when set", () => {
    sharedState.effort = {
      level: 4,
      name: "Ruthless",
      driver: "sonnet",
      thinking: "high",
      capped: false,
      extraction: "haiku",
      compaction: "haiku",
      cleavePreferLocal: false,
      cleaveFloor: "sonnet",
      reviewModel: "opus",
    } as any;

    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    const m = dashboard.operatorMetadata;
    assert.equal(m.effortName, "Ruthless");
    assert.equal(m.effortLevel, 4);
    assert.equal(m.driverTier, "sonnet");
    assert.equal(m.thinkingLevel, "high");
    assert.equal(m.effortCapped, false);
  });

  it("operatorMetadata reflects effortCapped when ceiling-locked", () => {
    sharedState.effort = {
      level: 3,
      name: "Substantial",
      driver: "haiku",
      thinking: "medium",
      capped: true,
      capLevel: 3,
      extraction: "haiku",
      compaction: "haiku",
      cleavePreferLocal: true,
      cleaveFloor: "haiku",
      reviewModel: "sonnet",
    } as any;

    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.operatorMetadata.effortCapped, true);
  });

  it("operatorMetadata.memoryTokenEstimate matches sharedState", () => {
    sharedState.memoryTokenEstimate = 3500;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.operatorMetadata.memoryTokenEstimate, 3500);
  });

  it("operatorMetadata exposes workingMemoryCount from last injection", () => {
    sharedState.lastMemoryInjection = {
      mode: "semantic" as any,
      projectFactCount: 20,
      globalFactCount: 5,
      workingMemoryFactCount: 8,
      semanticHitCount: 15,
      episodeCount: 3,
      edgeCount: 0,
      payloadChars: 4000,
      estimatedTokens: 1200,
    };
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    const m = dashboard.operatorMetadata;
    assert.equal(m.workingMemoryCount, 8);
    assert.equal(m.totalFactCount, 33); // 20 + 5 + 8
  });

  it("operatorMetadata.workingMemoryCount is null when no injection has occurred", () => {
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.operatorMetadata.workingMemoryCount, null);
    assert.equal(dashboard.operatorMetadata.totalFactCount, null);
  });

  it("operatorMetadata is JSON-serialisable", () => {
    sharedState.effort = {
      level: 5,
      name: "Lethal",
      driver: "opus",
      thinking: "high",
      capped: false,
      extraction: "sonnet",
      compaction: "sonnet",
      cleavePreferLocal: false,
      cleaveFloor: "sonnet",
      reviewModel: "opus",
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.doesNotThrow(() => JSON.stringify(dashboard.operatorMetadata));
  });
});

describe("dashboard snapshot — recovery actionability parity", () => {
  beforeEach(() => {
    sharedState.recovery = undefined;
  });

  it("recovery is null (not present) when no recovery event has occurred", () => {
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery, null);
  });

  it("recovery.actionable is true for 'escalate' action", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-opus",
      classification: "quota_exhausted",
      summary: "Quota exhausted, escalating",
      action: "escalate",
      timestamp: Date.now(),
      escalated: true,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery.actionable is true for 'retry' action", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "transient_server_error",
      summary: "5xx, retrying",
      action: "retry",
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery.actionable is true for 'switch_candidate' action", () => {
    sharedState.recovery = {
      provider: "openai",
      modelId: "gpt-4o",
      classification: "rate_limited",
      summary: "Rate limited, switching",
      action: "switch_candidate",
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery.actionable is true for 'switch_offline' action", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "authentication_failed",
      summary: "Auth failed, switching offline",
      action: "switch_offline",
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery.actionable is true for 'cooldown' action", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "rate_limited",
      summary: "Rate limited, cooling down",
      action: "cooldown",
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery.actionable is false for 'observe' action (non-actionable)", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "malformed_output",
      summary: "Detected malformed output, observing",
      action: "observe",
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, false);
  });

  it("recovery.actionable is true when escalated=true regardless of action", () => {
    sharedState.recovery = {
      provider: "anthropic",
      modelId: "claude-sonnet",
      classification: "unknown_upstream",
      summary: "Unknown error, escalated",
      action: "observe", // normally non-actionable
      timestamp: Date.now(),
      escalated: true, // but escalated overrides
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.equal(dashboard.recovery?.actionable, true);
  });

  it("recovery snapshot is JSON-serialisable", () => {
    sharedState.recovery = {
      provider: "openai",
      modelId: "gpt-4o",
      classification: "transient_server_error",
      summary: "Server error",
      action: "retry",
      retryCount: 1,
      maxRetries: 3,
      timestamp: Date.now(),
      escalated: false,
    } as any;
    const { dashboard } = buildControlPlaneState(REPO_ROOT, STARTED_AT);
    assert.doesNotThrow(() => JSON.stringify(dashboard.recovery));
    const parsed = JSON.parse(JSON.stringify(dashboard.recovery));
    assert.equal(typeof parsed.actionable, "boolean");
  });
});
