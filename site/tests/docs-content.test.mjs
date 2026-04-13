import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
const docsDir = resolve(here, '../src/pages/docs');

function readDoc(name) {
  return readFileSync(resolve(docsDir, name), 'utf8');
}

test('commands doc covers current slash command surface details', () => {
  const content = readDoc('commands.astro');

  assert.match(content, /\/copy \[mode\]/);
  assert.match(content, /raw<\/code>, <code>plain/);
  assert.match(content, /\/mouse \[mode\]/);
  assert.match(content, /\/context request/);
  assert.match(content, /\/skills \[action\]/);
  assert.match(content, /\/plugin \[action\]/);
  assert.match(content, /\/update install/);
  assert.match(content, /\/update channel/);
  assert.match(content, /\/auspex open/);
  assert.match(content, /compatibility\/debug browser path/);
});

test('migration doc avoids stale hard-coded project inventory snapshots', () => {
  const content = readDoc('migration.astro');

  assert.doesNotMatch(content, /Design Tree: 267 nodes/);
  assert.doesNotMatch(content, /60 open questions across 267 nodes/);
  assert.match(content, /Design tree data is live project state/);
  assert.match(content, /11 inference providers/);
  assert.match(content, /12 skills/);
});
