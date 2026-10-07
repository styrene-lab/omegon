const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');

// Compile the actual YAML literal, as github-script does. Do not maintain a
// second copy that could pass while the workflow itself has invalid syntax.
const workflow = fs.readFileSync(
  path.join(__dirname, '../.github/workflows/provider-drift.yml'), 'utf8',
);
const match = workflow.match(/          script: \|\n((?:            .*\n|\n)+)/);
assert.ok(match, 'provider drift github-script literal is missing');
const source = match[1].replace(/^            /gm, '');
assert.ok(!source.includes('${{'), 'pass workflow values via env/context, never JS interpolation');
const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
const reportIssue = new AsyncFunction('require', 'process', 'context', 'github', 'core', source);

async function runReporter(issues, report) {
  const calls = [];
  const notices = [];
  const api = Object.fromEntries(['listForRepo', 'createComment', 'update', 'create'].map(method => [
    method,
    async args => {
      calls.push({ method, ...args });
      return { data: method === 'listForRepo' ? issues : { number: 42 } };
    },
  ]));
  await reportIssue(
    name => {
      assert.equal(name, 'node:fs');
      return { readFileSync: (file, encoding) => {
        assert.equal(file, 'fixture-report.json');
        assert.equal(encoding, 'utf8');
        return JSON.stringify(report);
      } };
    },
    { env: { REPORT_PATH: 'fixture-report.json' } },
    { repo: { owner: 'fixture-owner', repo: 'fixture-repo' } },
    { rest: { issues: api } },
    { notice: text => notices.push(text) },
  );
  for (const call of calls) {
    assert.equal(call.owner, 'fixture-owner');
    assert.equal(call.repo, 'fixture-repo');
  }
  assert.deepEqual(calls[0], {
    method: 'listForRepo', owner: 'fixture-owner', repo: 'fixture-repo',
    state: 'open', labels: 'provider-drift', per_page: 100,
  });
  return { calls: calls.slice(1), notices };
}

// Deliberately include source-looking text. The reporter must treat every
// report field as data, including quotes, backticks, newlines, and dollar signs.
const hostile = '\'"`\\\n${throw new Error("evaluated report")} ${{ github.token }}';
const report = {
  fingerprint: `sha-${hostile}`,
  summary: `Summary\n${hostile}`,
  title: `Provider drift ${hostile}`,
  body: `Failure body\n${hostile}`,
};

test('creates a drift issue with report text preserved', async () => {
  const { calls, notices } = await runReporter([], report);
  assert.deepEqual(calls, [{
    method: 'create', owner: 'fixture-owner', repo: 'fixture-repo',
    title: report.title, body: report.body, labels: ['provider-drift', 'automated'],
  }]);
  assert.deepEqual(notices, ['Created provider drift issue #42']);
});

test('comments on an exact fingerprint without closing or creating issues', async () => {
  const { calls, notices } = await runReporter([
    { number: 10, body: null },
    { number: 11, body: `<!-- provider-drift-fingerprint: ${report.fingerprint} -->` },
  ], report);
  assert.deepEqual(calls, [{
    method: 'createComment', owner: 'fixture-owner', repo: 'fixture-repo',
    issue_number: 11, body: report.summary,
  }]);
  assert.deepEqual(notices, ['Updated existing provider drift issue #11']);
});

test('comments and closes stale fingerprints before creating a fresh issue', async () => {
  const { calls } = await runReporter([
    { number: 10, body: null }, { number: 11, body: 'old fingerprint' },
  ], report);
  assert.deepEqual(calls.map(call => [call.method, call.issue_number]), [
    ['createComment', 10], ['update', 10],
    ['createComment', 11], ['update', 11], ['create', undefined],
  ]);
  for (const call of calls.filter(call => call.method === 'createComment')) {
    assert.equal(call.body,
      'Superseded by drift fingerprint `' + report.fingerprint +
      '`. Closing in favor of a fresh issue for the new failure shape.');
  }
  for (const call of calls.filter(call => call.method === 'update')) {
    assert.equal(call.state, 'closed');
  }
  assert.equal(calls.at(-1).body, report.body);
});
