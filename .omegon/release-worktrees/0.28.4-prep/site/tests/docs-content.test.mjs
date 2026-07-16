import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';

const here = dirname(fileURLToPath(import.meta.url));
const docsDir = resolve(here, '../src/pages/docs');
const npmRunBuild = process.platform === 'win32'
  ? { command: process.env.ComSpec ?? 'cmd.exe', args: ['/d', '/s', '/c', 'npm run build'] }
  : { command: 'npm', args: ['run', 'build'] };
const buildEnvironment = { ...process.env, FORCE_COLOR: '0' };
if (process.platform === 'win32') {
  buildEnvironment.CI = 'true';
}

function readDoc(name) {
  return readFileSync(resolve(docsDir, name), 'utf8');
}

test('install docs use canonical snippets for all channels', () => {
  const content = readDoc('install.astro');

  // Uses snippet system, not hardcoded commands
  assert.match(content, /snippet\("install\.quick_install"\)/);
  assert.match(content, /snippet\("install\.install_nightly"\)/);
  assert.match(content, /snippet\("install\.install_version"\)/);
  assert.doesNotMatch(content, /omegon\.styrene\.dev/);
  // Auth commands use correct form
  assert.match(content, /snippet\("auth\.login_anthropic"\)/);
  assert.doesNotMatch(content, /omegon login(?! )/);
});

test('homepage has version selector and install section', () => {
  const content = readFileSync(resolve(here, '../src/pages/index.astro'), 'utf8');

  assert.match(content, /version-select/);
  assert.match(content, /Stable/);
  assert.match(content, /Nightly/);
  assert.match(content, /install-cmd/);
  assert.match(content, /copy-btn/);
  assert.doesNotMatch(content, /omegon\.styrene\.dev/);
});

test('privacy page uses canonical site label', () => {
  const content = readFileSync(resolve(here, '../src/pages/privacy.astro'), 'utf8');

  assert.match(content, /siteLabel/);
  assert.match(content, /omegon\.styrene\.io/);
});

test('extensions page uses extension init, not extension new', () => {
  const content = readDoc('extensions.astro');

  assert.match(content, /snippet\("cli\.extension_init"\)/);
  assert.doesNotMatch(content, /extension new/);
});

test('no page imports siteVariant', () => {
  const pages = readdirSync(docsDir).filter(f => f.endsWith('.astro'));
  for (const page of pages) {
    const content = readDoc(page);
    assert.doesNotMatch(content, /siteVariant/, `${page} still imports siteVariant`);
  }
});

test('site builds successfully', () => {
  execFileSync(npmRunBuild.command, npmRunBuild.args, {
    cwd: resolve(here, '..'),
    env: buildEnvironment,
    stdio: 'pipe',
  });

  const changelogHtml = readFileSync(resolve(here, '../dist/changelog/index.html'), 'utf8');
  const privacyHtml = readFileSync(resolve(here, '../dist/privacy/index.html'), 'utf8');
  const termsHtml = readFileSync(resolve(here, '../dist/terms/index.html'), 'utf8');
  const rootChangelog = readFileSync(resolve(here, '../../CHANGELOG.md'), 'utf8');

  assert.match(rootChangelog, /^\+\+\+/);
  for (const rendered of [changelogHtml, privacyHtml, termsHtml]) {
    assert.doesNotMatch(rendered, /imported_reference/);
    assert.doesNotMatch(rendered, /\[publication\]/);
    assert.doesNotMatch(rendered, /^\+\+\+/m);
    assert.doesNotMatch(rendered, /^---$/m);
  }
  assert.match(changelogHtml, /local sandbox evidence-substrate smoke suite/);
  for (const version of [
    '0.19.6',
    '0.19.5',
    '0.19.4',
    '0.19.3',
    '0.19.2',
    '0.19.1',
    '0.19.0',
    '0.18.6',
    '0.18.5',
  ]) {
    assert.match(changelogHtml, new RegExp(`\\[${version}\\]`));
  }
});
