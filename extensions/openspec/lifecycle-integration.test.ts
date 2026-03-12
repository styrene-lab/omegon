import { afterEach, beforeEach, describe, it } from "node:test";
import assert from "node:assert/strict";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { execFileSync } from "node:child_process";

import openspecExtension from "./index.ts";
import { createChange } from "./spec.ts";
import { generateFrontmatter } from "../design-tree/tree.ts";
import type { DesignNode } from "../design-tree/types.ts";

function writeDesignDoc(docsDir: string, id: string, openspecChange: string): void {
  const node: DesignNode = {
    id,
    title: `Test ${id}`,
    status: "implementing",
    dependencies: [],
    related: [],
    tags: [],
    open_questions: [],
    branches: [],
    openspec_change: openspecChange,
    filePath: path.join(docsDir, `${id}.md`),
    lastModified: Date.now(),
  };
  const content = `${generateFrontmatter(node)}\n# ${node.title}\n\n## Overview\n\nTest node.\n`;
  fs.writeFileSync(node.filePath, content);
}

function createFakePi() {
  const commands = new Map<string, any>();
  const sentMessages: any[] = [];
  const tools: any[] = [];
  const messageRenderers = new Map<string, any>();
  const eventHandlers = new Map<string, any[]>();

  return {
    commands,
    sentMessages,
    tools,
    messageRenderers,
    events: {
      emit() {},
    },
    registerTool(tool: any) {
      tools.push(tool);
    },
    registerCommand(name: string, command: any) {
      commands.set(name, command);
    },
    registerMessageRenderer(name: string, renderer: any) {
      messageRenderers.set(name, renderer);
    },
    on(event: string, handler: any) {
      const handlers = eventHandlers.get(event) ?? [];
      handlers.push(handler);
      eventHandlers.set(event, handlers);
    },
    async sendMessage(message: any) {
      sentMessages.push(message);
    },
    async exec(command: string, args: string[], opts: { cwd: string }) {
      try {
        const stdout = execFileSync(command, args, {
          cwd: opts.cwd,
          encoding: "utf-8",
          stdio: ["ignore", "pipe", "pipe"],
        });
        return { code: 0, stdout, stderr: "" };
      } catch (error: any) {
        return {
          code: error.status ?? 1,
          stdout: error.stdout?.toString?.() ?? "",
          stderr: error.stderr?.toString?.() ?? "",
        };
      }
    },
  };
}

describe("openspec lifecycle integration", () => {
  let tmpDir: string;
  let pi: ReturnType<typeof createFakePi>;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "openspec-lifecycle-"));
    execFileSync("git", ["init"], { cwd: tmpDir, encoding: "utf-8" });
    execFileSync("git", ["config", "user.name", "Test User"], { cwd: tmpDir, encoding: "utf-8" });
    execFileSync("git", ["config", "user.email", "test@example.com"], { cwd: tmpDir, encoding: "utf-8" });
    fs.writeFileSync(path.join(tmpDir, "README.md"), "# test\n");
    execFileSync("git", ["add", "README.md"], { cwd: tmpDir, encoding: "utf-8" });
    execFileSync("git", ["commit", "-m", "init"], { cwd: tmpDir, encoding: "utf-8" });

    const change = createChange(tmpDir, "my-change", "My Change", "Intent");
    fs.mkdirSync(path.join(change.changePath, "specs"), { recursive: true });
    fs.writeFileSync(path.join(change.changePath, "specs", "core.md"), `# core — Delta Spec\n\n## ADDED Requirements\n\n### Requirement: Demo\n\n#### Scenario: Happy path\nGiven x\nWhen y\nThen z\n`);
    fs.writeFileSync(path.join(change.changePath, "tasks.md"), "## 1. Demo\n- [x] 1.1 Done\n");

    const docsDir = path.join(tmpDir, "docs");
    fs.mkdirSync(docsDir, { recursive: true });
    writeDesignDoc(docsDir, "my-change", "my-change");

    execFileSync("git", ["add", "."], { cwd: tmpDir, encoding: "utf-8" });
    execFileSync("git", ["commit", "-m", "scaffold change"], { cwd: tmpDir, encoding: "utf-8" });

    pi = createFakePi();
    openspecExtension(pi as any);
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  async function runCommand(name: string, args: string) {
    const command = pi.commands.get(name);
    assert.ok(command, `missing command ${name}`);
    const notifications: Array<{ text: string; level: string }> = [];
    const ctx = {
      cwd: tmpDir,
      ui: {
        notify(text: string, level: string) {
          notifications.push({ text, level });
        },
      },
    };
    await command.handler(args, ctx);
    return { notifications, sentMessages: pi.sentMessages };
  }

  async function runTool(params: Record<string, unknown>, cwd = tmpDir) {
    const tool = pi.tools.find((entry: any) => entry.name === "openspec_manage");
    assert.ok(tool, "missing openspec_manage tool");
    return await tool.execute("tool-1", params, {} as any, () => {}, { cwd });
  }

  async function persistAssessment(
    outcome: "pass" | "reopen" | "ambiguous",
    options?: { cwd?: string; summary?: string; changedFiles?: string[]; constraints?: string[] },
  ) {
    return await runTool({
      action: "reconcile_after_assess",
      change_name: "my-change",
      assessment_kind: "spec",
      outcome,
      summary: options?.summary,
      changed_files: options?.changedFiles,
      constraints: options?.constraints,
    }, options?.cwd ?? tmpDir);
  }

  it("surfaces archive-ready during /opsx:verify when assessment and lifecycle state are current", async () => {
    await persistAssessment("pass");

    const result = await runCommand("opsx:verify", "my-change");
    assert.equal(result.sentMessages.length, 0);
    assert.equal(result.notifications.length, 1);
    assert.match(result.notifications[0].text, /Verification state for 'my-change': archive-ready/);
    assert.match(result.notifications[0].text, /Next: \/opsx:archive my-change/);
    assert.match(result.notifications[0].text, /Outcome: pass/);
    assert.equal(result.notifications[0].level, "info");
  });

  it("surfaces verification substates in lifecycle status and get output", async () => {
    let status = await runTool({ action: "status" });
    const missingText = status.content[0].text as string;
    assert.match(missingText, /\(verifying\)/);
    assert.match(missingText, /Verification: missing-assessment/);
    assert.match(missingText, /Next: \/assess spec my-change/);
    assert.equal(status.details.changes[0].verificationSubstate, "missing-assessment");

    await persistAssessment("pass");
    status = await runTool({ action: "status" });
    const readyText = status.content[0].text as string;
    assert.match(readyText, /Verification: archive-ready/);
    assert.match(readyText, /Next: \/opsx:archive my-change/);
    assert.equal(status.details.changes[0].stage, "verifying");
    assert.equal(status.details.changes[0].verificationStage, "verifying");
    assert.equal(status.details.changes[0].verificationSubstate, "archive-ready");

    const getResult = await runTool({ action: "get", change_name: "my-change" });
    const getText = getResult.content[0].text as string;
    assert.match(getText, /\*\*Verification substate:\*\* archive-ready/);
    assert.match(getText, /Next: \/opsx:archive my-change/);
  });

  it("requests refreshed assessment during /opsx:verify when persisted state is stale", async () => {
    await persistAssessment("pass");

    fs.appendFileSync(path.join(tmpDir, "openspec", "changes", "my-change", "tasks.md"), "- [x] 1.2 Still done\n");
    pi.sentMessages.length = 0;

    const result = await runCommand("opsx:verify", "my-change");
    assert.equal(result.notifications.length, 0);
    assert.equal(result.sentMessages.length, 1);
    assert.match(result.sentMessages[0].content, /Verification state: stale-assessment/);
    assert.match(result.sentMessages[0].content, /Implementation snapshot fingerprint differs/);
    assert.match(result.sentMessages[0].content, /Run `\/assess spec my-change` now/);
  });

  it("surfaces reopened-work during /opsx:verify instead of collapsing to a rerun prompt", async () => {
    await persistAssessment("reopen", { summary: "Follow-up work remains" });
    pi.sentMessages.length = 0;

    const result = await runCommand("opsx:verify", "my-change");
    assert.equal(result.sentMessages.length, 0);
    assert.equal(result.notifications.length, 1);
    assert.match(result.notifications[0].text, /Verification state for 'my-change': reopened-work/);
    assert.match(result.notifications[0].text, /Complete follow-up work for my-change/);
    assert.match(result.notifications[0].text, /Outcome: reopen/);
    assert.equal(result.notifications[0].level, "warning");
  });

  it("surfaces archive-ready (not missing-binding) when lifecycle bindings are stale — binding is now informational", async () => {
    await persistAssessment("pass");
    fs.rmSync(path.join(tmpDir, "docs", "my-change.md"));

    const status = await runTool({ action: "status" });
    const statusText = status.content[0].text as string;
    // missing_design_binding is now isError:false (informational) — no longer blocks archive
    assert.match(statusText, /Verification: archive-ready/);

    pi.sentMessages.length = 0;
    const result = await runCommand("opsx:verify", "my-change");
    assert.equal(result.sentMessages.length, 0);
    assert.equal(result.notifications.length, 1);
    assert.match(result.notifications[0].text, /Verification state for 'my-change': archive-ready/);
    assert.equal(result.notifications[0].level, "info");
  });

  it("refuses /opsx:archive when assessment is missing and succeeds on current explicit pass", async () => {
    const blocked = await runCommand("opsx:archive", "my-change");
    assert.equal(blocked.notifications.length, 1);
    assert.match(blocked.notifications[0].text, /no persisted assessment record exists/i);
    assert.equal(blocked.notifications[0].level, "warning");

    pi.sentMessages.length = 0;
    await persistAssessment("pass");

    const allowed = await runCommand("opsx:archive", "my-change");
    assert.equal(allowed.notifications.length, 1);
    assert.match(allowed.notifications[0].text, /Archived 'my-change'/);
    assert.equal(allowed.notifications[0].level, "info");
    assert.ok(fs.existsSync(path.join(tmpDir, "openspec", "archive")));
  });

  it("refuses /opsx:archive for ambiguous, reopened, and stale assessment states", async () => {
    await persistAssessment("ambiguous");
    const ambiguous = await runCommand("opsx:archive", "my-change");
    assert.equal(ambiguous.notifications.length, 1);
    assert.match(ambiguous.notifications[0].text, /latest persisted assessment is ambiguous/i);
    assert.equal(ambiguous.notifications[0].level, "warning");

    pi = createFakePi();
    openspecExtension(pi as any);
    await persistAssessment("reopen", { summary: "Follow-up work remains" });
    const reopened = await runCommand("opsx:archive", "my-change");
    assert.equal(reopened.notifications.length, 1);
    assert.match(reopened.notifications[0].text, /reopened work/i);
    assert.equal(reopened.notifications[0].level, "warning");

    pi = createFakePi();
    openspecExtension(pi as any);
    await persistAssessment("pass");
    fs.appendFileSync(path.join(tmpDir, "openspec", "changes", "my-change", "tasks.md"), "\n");

    const stale = await runCommand("opsx:archive", "my-change");
    assert.equal(stale.notifications.length, 1);
    assert.match(stale.notifications[0].text, /implementation snapshot fingerprint differs/i);
    assert.equal(stale.notifications[0].level, "warning");
  });
});
