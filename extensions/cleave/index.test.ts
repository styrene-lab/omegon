import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { describe, it } from "node:test";

function runJsonScript(script: string) {
	return JSON.parse(execFileSync("node", ["-e", script], {
		cwd: process.cwd(),
		encoding: "utf-8",
		stdio: ["ignore", "pipe", "pipe"],
	}));
}

function runAssessSpecScenario(mode: "bridged" | "interactive" | "reopen") {
	const script = String.raw`
(async () => {
  const fs = await import('node:fs');
  const path = await import('node:path');
  const { createAssessStructuredExecutors } = await import('./extensions/cleave/index.ts');
  const mode = ${JSON.stringify(mode)};
  const changeName = '__test-assess-fixture';

  // Scaffold a temporary OpenSpec change so findExecutableChanges discovers it
  const fixtureDir = path.join(process.cwd(), 'openspec', 'changes', changeName);
  const specsDir = path.join(fixtureDir, 'specs', 'test');
  fs.mkdirSync(specsDir, { recursive: true });
  const NL = String.fromCharCode(10);
  fs.writeFileSync(path.join(fixtureDir, 'proposal.md'), ['# Test fixture', '## Intent', 'Test', ''].join(NL));
  fs.writeFileSync(path.join(fixtureDir, 'tasks.md'), ['# Tasks', '- [x] 1.1 Done', ''].join(NL));
  fs.writeFileSync(path.join(specsDir, 'spec.md'), ['# test/spec', '## ADDED Requirements', '### Requirement: Test', '#### Scenario: test passes', 'Given a test', 'When it runs', 'Then it passes', ''].join(NL));
  const cleanup = () => { try { fs.rmSync(fixtureDir, { recursive: true, force: true }); } catch {} };
  const scenarios = [
    { domain: 'test/spec', requirement: 'Test', scenario: 'test passes', status: mode === 'reopen' ? 'FAIL' : 'PASS', evidence: ['extensions/model-budget.ts'], notes: mode === 'reopen' ? 'Reopened work.' : undefined },
  ];
  let runnerCalled = false;
  const pi = {
    exec: async (_cmd, args) => {
      if (args[0] === 'rev-parse') return { code: 0, stdout: 'abc123\n', stderr: '' };
      if (args[0] === 'status') return { code: 0, stdout: '', stderr: '' };
      if (args[0] === 'diff') return { code: 0, stdout: '', stderr: '' };
      return { code: 0, stdout: '', stderr: '' };
    },
  };
  const executors = createAssessStructuredExecutors(pi, {
    runSpecAssessment: async () => {
      runnerCalled = true;
      if (mode === 'interactive') {
        throw new Error('interactive assess should not invoke the bridged runner');
      }
      return {
        assessed: {
          summary: mode === 'reopen'
            ? { total: 1, pass: 0, fail: 1, unclear: 0 }
            : { total: 1, pass: 1, fail: 0, unclear: 0 },
          scenarios,
          changedFiles: mode === 'reopen' ? ['extensions/cleave/index.ts'] : [],
          constraints: mode === 'reopen'
            ? ['Lifecycle metadata must be derived after scenario evaluation']
            : ['Bridge result must remain authoritative in-band'],
        },
      };
    },
  });
  const ctx = mode === 'interactive'
    ? { cwd: process.cwd(), hasUI: true, waitForIdle: async () => {}, model: { id: 'test-model' } }
    : { cwd: process.cwd(), bridgeInvocation: true, hasUI: false, model: { id: 'test-model' } };
  const result = await executors.spec(changeName, ctx);
  process.stdout.write(JSON.stringify({
    summary: result.summary,
    completion: result.completion,
    lifecycleOutcome: result.lifecycleRecord?.outcome,
    effectTypes: result.effects.map((effect) => effect.type),
    recommendedReconcileOutcome: result.data?.recommendedReconcileOutcome,
    reopen: result.lifecycleRecord?.reconciliation.reopen,
    changedFiles: result.lifecycleRecord?.reconciliation.changedFiles,
    constraints: result.lifecycleRecord?.reconciliation.constraints,
    runnerCalled,
  }));
  cleanup();
})();
`;

	return runJsonScript(script);
}

function runDirtyTreePreflightScenario(mode: "clean" | "volatile-only" | "checkpoint" | "generic" | "unknowns") {
	const script = String.raw`
(async () => {
  const { runDirtyTreePreflight } = await import('./extensions/cleave/index.ts');
  const mode = ${JSON.stringify(mode)};
  const commands = [];
  const updates = [];
  const answersByMode = {
    clean: [],
    'volatile-only': [],
    checkpoint: ['checkpoint', ''],
    generic: ['proceed-without-cleave'],
    unknowns: ['stash-unrelated'],
  };
  const inputs = [...answersByMode[mode]];
  const pi = {
    exec: async (cmd, args) => {
      commands.push([cmd, ...args]);
      if (cmd !== 'git') return { code: 0, stdout: '', stderr: '' };
      if (args[0] === 'status') {
        const stdoutByMode = {
          clean: '',
          'volatile-only': ' M .pi/memory/facts.jsonl\n',
          checkpoint: ' M openspec/changes/cleave-dirty-tree-checkpointing/tasks.md\n?? docs/cleave-dirty-tree-checkpointing.md\n',
          generic: ' M README.md\n',
          unknowns: ' M openspec/changes/other-change/tasks.md\n?? scratch/notes.md\n',
        };
        return { code: 0, stdout: stdoutByMode[mode], stderr: '' };
      }
      if (args[0] === 'add' || args[0] === 'commit' || args[0] === 'stash') {
        return { code: 0, stdout: '', stderr: '' };
      }
      return { code: 0, stdout: '', stderr: '' };
    },
  };
  const result = await runDirtyTreePreflight(pi, {
    repoPath: process.cwd(),
    openspecChangePath: mode === 'generic' ? undefined : 'openspec/changes/cleave-dirty-tree-checkpointing',
    onUpdate: (payload) => updates.push(payload),
    ui: mode === 'clean' || mode === 'volatile-only'
      ? undefined
      : { input: async (_prompt, initial) => inputs.shift() ?? initial ?? '' },
  });
  process.stdout.write(JSON.stringify({ result, commands, updates }));
})();
`;

	return runJsonScript(script);
}

describe("createAssessStructuredExecutors", () => {
	it("returns a completed in-band result for bridged /assess spec", () => {
		const result = runAssessSpecScenario("bridged");

		assert.match(result.summary, /completed spec assessment/i);
		assert.deepEqual(result.completion, {
			completed: true,
			completedInBand: true,
			requiresFollowUp: false,
			outcome: "pass",
		});
		assert.equal(result.lifecycleOutcome, "pass");
		assert.deepEqual(result.effectTypes, ["reconcile_hint"]);
		assert.equal(result.recommendedReconcileOutcome, "pass");
		assert.equal(result.runnerCalled, true);
	});

	it("keeps interactive /assess spec follow-up driven", () => {
		const result = runAssessSpecScenario("interactive");

		assert.match(result.summary, /prepared spec assessment/i);
		assert.deepEqual(result.completion, {
			completed: false,
			completedInBand: false,
			requiresFollowUp: true,
		});
		assert.deepEqual(result.effectTypes, ["view", "follow_up", "reconcile_hint"]);
		assert.equal(result.runnerCalled, false);
	});

	it("derives bridged lifecycle metadata from the completed assessment result", () => {
		const result = runAssessSpecScenario("reopen");

		assert.equal(result.completion.outcome, "reopen");
		assert.equal(result.lifecycleOutcome, "reopen");
		assert.equal(result.recommendedReconcileOutcome, "reopen");
		assert.equal(result.reopen, true);
		assert.deepEqual(result.changedFiles, ["extensions/cleave/index.ts"]);
		assert.deepEqual(result.constraints, ["Lifecycle metadata must be derived after scenario evaluation"]);
	});
});

describe("dirty-tree preflight acceptance coverage", () => {
	it("clean tree proceeds without a dirty-tree checkpoint prompt", () => {
		const result = runDirtyTreePreflightScenario("clean");
		assert.equal(result.result, "continue");
		assert.deepEqual(result.updates, []);
		assert.deepEqual(result.commands, [["git", "status", "--porcelain"]]);
	});

	it("dirty tree summary distinguishes related, unrelated or unknown, and volatile files", () => {
		const result = runDirtyTreePreflightScenario("unknowns");
		const summary = result.updates[0]?.content?.[0]?.text ?? "";
		assert.match(summary, /related changes/i);
		assert.match(summary, /unrelated \/ unknown changes/i);
		assert.match(summary, /volatile artifacts/i);
		assert.match(summary, /openspec\/changes\/other-change\/tasks\.md/);
		assert.match(summary, /scratch\/notes\.md/);
	});

	it("volatile-only dirt does not block cleave by default", () => {
		const result = runDirtyTreePreflightScenario("volatile-only");
		const summary = result.updates[0]?.content?.[0]?.text ?? "";
		const autoSummary = result.updates[1]?.content?.[0]?.text ?? "";
		assert.equal(result.result, "continue");
		assert.match(summary, /volatile artifacts/i);
		assert.match(autoSummary, /stashed volatile artifacts automatically/i);
		assert.doesNotMatch(summary, /interactive input is unavailable/i);
		assert.equal(result.commands.filter((command: string[]) => command[1] === "stash").length, 1);
	});

	it("low-confidence unknown files are excluded from checkpoint scope by default", () => {
		const result = runDirtyTreePreflightScenario("unknowns");
		const stashCommand = result.commands.find((command: string[]) => command[1] === "stash");
		assert.equal(result.result, "continue");
		assert.ok(stashCommand, "expected stash command for unrelated/unknown files");
		assert.ok(stashCommand.includes("openspec/changes/other-change/tasks.md"));
		assert.ok(stashCommand.includes("scratch/notes.md"));
		assert.equal(result.commands.some((command: string[]) => command[1] === "add"), false);
		assert.equal(result.commands.some((command: string[]) => command[1] === "commit"), false);
	});

	it("generic classification still works when no active OpenSpec change exists", () => {
		const result = runDirtyTreePreflightScenario("generic");
		const summary = result.updates[0]?.content?.[0]?.text ?? "";
		assert.equal(result.result, "skip_cleave");
		assert.match(summary, /generic preflight fallback without OpenSpec context/i);
		assert.match(summary, /proceed-without-cleave/i);
	});

	it("checkpoint plans stage related files and wait for explicit approval before committing", () => {
		const result = runDirtyTreePreflightScenario("checkpoint");
		const summary = result.updates[0]?.content?.[0]?.text ?? "";
		const addCommand = result.commands.find((command: string[]) => command[1] === "add");
		const commitCommand = result.commands.find((command: string[]) => command[1] === "commit");
		assert.equal(result.result, "continue");
		assert.match(summary, /Suggested checkpoint commit:/i);
		assert.ok(addCommand, "expected git add to stage checkpoint files");
		assert.ok(commitCommand, "expected git commit after explicit approval");
		assert.ok(addCommand.includes("openspec/changes/cleave-dirty-tree-checkpointing/tasks.md"));
		assert.ok(!addCommand.includes("docs/cleave-dirty-tree-checkpointing.md"));
		assert.ok(commitCommand.some((part: string) => /checkpoint/i.test(String(part))));
	});
});
