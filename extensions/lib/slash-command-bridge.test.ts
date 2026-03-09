import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  buildSlashCommandResult,
  createSlashCommandBridge,
  type BridgedSlashCommand,
} from "./slash-command-bridge.ts";

function makeFakePi() {
  const commands = new Map<string, any>();
  return {
    commands,
    registerCommand(name: string, command: any) {
      commands.set(name, command);
    },
  };
}

describe("slash-command-bridge", () => {
  it("executes allowlisted commands through the structured executor", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();
    let receivedArgs = "";

    bridge.register(pi as any, {
      name: "inspect",
      description: "Inspect workspace state",
      bridge: {
        agentCallable: true,
        sideEffectClass: "read",
        resultContract: "demo.inspect.v1",
      },
      structuredExecutor: async (args) => {
        receivedArgs = args;
        return buildSlashCommandResult("inspect", args.split(/\s+/).filter(Boolean), {
          ok: true,
          summary: "Inspection complete",
          humanText: "Inspection complete",
          data: { mode: "bridge" },
          effects: { sideEffectClass: "read" },
          nextSteps: [{ label: "Continue" }],
        });
      },
    } satisfies BridgedSlashCommand<{ mode: string }>);

    const result = await bridge.execute({ command: "inspect", args: ["alpha", "beta"] }, {} as any);

    assert.equal(receivedArgs, "alpha beta");
    assert.equal(result.ok, true);
    assert.deepEqual(result.args, ["alpha", "beta"]);
    assert.equal(result.effects.sideEffectClass, "read");
    assert.deepEqual(result.data, { mode: "bridge" });
  });

  it("refuses commands that are not allowlisted for agents", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();

    bridge.register(pi as any, {
      name: "operator-only",
      description: "Operator-only action",
      bridge: {
        agentCallable: false,
        sideEffectClass: "workspace-write",
      },
      structuredExecutor: async () => {
        throw new Error("should not execute");
      },
    } satisfies BridgedSlashCommand);

    const result = await bridge.execute({ command: "operator-only" }, {} as any);

    assert.equal(result.ok, false);
    assert.equal(result.confirmationRequired, undefined);
    assert.match(result.summary, /not approved for agent invocation/i);
    assert.match(result.humanText, /not allowlisted/i);
    assert.equal(result.effects.sideEffectClass, "workspace-write");
  });

  it("surfaces confirmation-required responses without executing the command", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();
    let executed = false;

    bridge.register(pi as any, {
      name: "publish",
      description: "Publish externally",
      bridge: {
        agentCallable: true,
        sideEffectClass: "external-side-effect",
        requiresConfirmation: true,
      },
      structuredExecutor: async () => {
        executed = true;
        return buildSlashCommandResult("publish", [], {
          ok: true,
          summary: "published",
          humanText: "published",
          effects: { sideEffectClass: "external-side-effect" },
        });
      },
    } satisfies BridgedSlashCommand);

    const blocked = await bridge.execute({ command: "publish" }, {} as any);
    assert.equal(blocked.ok, false);
    assert.equal(blocked.confirmationRequired, true);
    assert.equal(executed, false);
    assert.match(blocked.humanText, /requires operator confirmation/i);

    const allowed = await bridge.execute({ command: "publish", confirmed: true }, {} as any);
    assert.equal(allowed.ok, true);
    assert.equal(executed, true);
  });

  it("interactive registration and bridged execution share the same structured executor", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();
    const notifications: Array<{ text: string; level: string }> = [];

    const structuredExecutor = async (args: string) => {
      const tokens = args.split(/\s+/).filter(Boolean);
      const mode = tokens.length <= 3 ? "execute" : "cleave";
      return buildSlashCommandResult("assess-complexity", tokens, {
        ok: true,
        summary: `Complexity decision: ${mode}`,
        humanText: mode === "execute" ? "Execute directly" : "Decomposition recommended",
        data: { decision: mode, tokenCount: tokens.length },
        effects: { sideEffectClass: "read" },
        nextSteps: [{ label: mode === "execute" ? "Execute directly" : "Run /cleave" }],
      });
    };

    bridge.register(pi as any, {
      name: "assess-complexity",
      description: "Assess complexity via shared executor",
      bridge: {
        agentCallable: true,
        sideEffectClass: "read",
        resultContract: "assess.complexity.v1",
      },
      structuredExecutor,
    } satisfies BridgedSlashCommand<{ decision: string; tokenCount: number }>);

    const registered = pi.commands.get("assess-complexity");
    assert.ok(registered);

    await registered.handler("rename helper function", {
      ui: { notify: (text: string, level: string) => notifications.push({ text, level }) },
    });
    const bridged = await bridge.execute({ command: "assess-complexity", args: ["rename", "helper", "function"] }, {} as any);

    assert.equal(notifications.length, 1);
    assert.equal(notifications[0].text, bridged.summary);
    assert.equal(notifications[0].level, "info");
    assert.equal(bridged.ok, true);
    assert.deepEqual(bridged.data, { decision: "execute", tokenCount: 3 });
    assert.match(bridged.humanText, /execute directly/i);
  });

  it("tool wrapper returns structured results and available command metadata", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();

    bridge.register(pi as any, {
      name: "inspect",
      description: "Inspect workspace state",
      bridge: {
        agentCallable: true,
        sideEffectClass: "read",
        resultContract: "demo.inspect.v1",
        summary: "Inspection command",
      },
      structuredExecutor: async () => buildSlashCommandResult("inspect", [], {
        ok: true,
        summary: "Inspection complete",
        humanText: "Inspection complete",
        data: { ok: true },
        effects: { sideEffectClass: "read" },
      }),
    } satisfies BridgedSlashCommand);

    const tool = bridge.createToolDefinition();
    const response: any = await tool.execute("tool-1", { command: "inspect" }, {} as any, () => {}, {} as any);

    assert.equal(response.isError, false);
    assert.equal(response.details.result.command, "inspect");
    assert.equal(response.details.availableCommands.length, 1);
    assert.equal(response.details.availableCommands[0].bridge.resultContract, "demo.inspect.v1");
    assert.equal(response.content[0].type, "text");
  });

  it("preserves nested lifecycle assessment metadata through bridged execution", async () => {
    const bridge = createSlashCommandBridge();
    const pi = makeFakePi();

    bridge.register(pi as any, {
      name: "assess",
      description: "Structured assessment",
      bridge: {
        agentCallable: true,
        sideEffectClass: "read",
        resultContract: "assess.lifecycle.v1",
      },
      structuredExecutor: async () => buildSlashCommandResult("assess", ["spec", "my-change"], {
        ok: true,
        summary: "Assessment recorded",
        humanText: "Assessment recorded",
        data: {
          changeName: "my-change",
          assessmentKind: "spec",
          outcome: "pass",
          snapshot: {
            gitHead: "abc123",
            fingerprint: "fingerprint",
            dirty: false,
          },
          reconciliation: {
            reopen: false,
            changedFiles: ["extensions/openspec/index.ts"],
            constraints: ["Archive stays fail-closed"],
            recommendedAction: null,
          },
        },
        effects: {
          sideEffectClass: "read",
          filesChanged: ["openspec/changes/my-change/assessment.json"],
          lifecycleTouched: ["openspec"],
        },
      }),
    } satisfies BridgedSlashCommand);

    const result = await bridge.execute({ command: "assess" }, {} as any);
    assert.equal(result.ok, true);
    assert.deepEqual(result.data, {
      changeName: "my-change",
      assessmentKind: "spec",
      outcome: "pass",
      snapshot: {
        gitHead: "abc123",
        fingerprint: "fingerprint",
        dirty: false,
      },
      reconciliation: {
        reopen: false,
        changedFiles: ["extensions/openspec/index.ts"],
        constraints: ["Archive stays fail-closed"],
        recommendedAction: null,
      },
    });
    assert.deepEqual(result.effects.filesChanged, ["openspec/changes/my-change/assessment.json"]);
    assert.deepEqual(result.effects.lifecycleTouched, ["openspec"]);
  });
});
