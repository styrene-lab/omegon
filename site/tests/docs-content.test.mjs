import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { parseSnippetYaml } from '../scripts/load-snippets.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const docsDir = resolve(here, '../src/pages/docs');
const snippetsDir = resolve(here, '../snippets');
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

function docsPages(dir = docsDir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap(entry => {
    const path = resolve(dir, entry.name);
    return entry.isDirectory() ? docsPages(path) : entry.name.endsWith('.astro') ? [path] : [];
  });
}

test('install docs use canonical snippets for all channels', () => {
  const content = readDoc('install.astro');

  // Uses snippet system, not hardcoded commands
  assert.match(content, /snippet\("install\.quick_install"\)/);
  assert.match(content, /snippet\("install\.install_nightly"\)/);
  assert.match(content, /snippet\("install\.install_version"\)/);
  assert.match(content, /snippet\("install\.verify_companions"\)/);
  assert.match(content, /snippet\("verify\.release_verify"\)/);
  assert.match(content, /href="\/docs\/recovery"/);
  assert.doesNotMatch(content, /omegon\.styrene\.dev/);
  // Auth commands use correct form
  assert.match(content, /snippet\("auth\.login_anthropic"\)/);
  assert.doesNotMatch(content, /omegon login(?! )/);
});

test('public docs internal links resolve to checked-in pages', () => {
  const pages = docsPages();
  const routes = new Set(pages.map(path => {
    const relative = path.slice(docsDir.length).replaceAll('\\', '/').replace(/\.astro$/, '');
    return `/docs${relative}`.replace(/\/index$/, '').replace(/\/$/, '') || '/docs';
  }));

  for (const page of pages) {
    const content = readFileSync(page, 'utf8');
    for (const match of content.matchAll(/href=["'](\/docs(?:\/[^"'#?]*)?)/g)) {
      const route = match[1].replace(/\/$/, '') || '/docs';
      assert.ok(routes.has(route), `${page} links to missing internal route ${route}`);
    }
  }
});

test('public docs explain content-pack packaging, trust, and boot generation', () => {
  const install = readDoc('install.astro');
  const skills = readDoc('skills.astro');
  const plugins = readDoc('plugins.astro');
  assert.match(install, /share\/omegon\/content-packs/);
  assert.match(install, /next process boot/);
  assert.match(skills, /Residency does not grant tool access/);
  assert.match(skills, /Project content overrides user content/);
  assert.match(plugins, /pins one content generation at boot/);
  assert.match(plugins, /never grants prompt, tool, effect, executable, or path authority/);
  assert.match(plugins, /retains its host-owned common policy/);
  assert.match(plugins, /separate host-owned common policy/);
  assert.match(plugins, /inventoried supporting references/);
  assert.match(plugins, /model-driven session compaction is\s+disabled locally/);
});

test('public optional-domain docs state local absence behavior', () => {
  const index = readDoc('index.astro');
  const openspec = readDoc('openspec.astro');
  const memory = readDoc('memory.astro');
  const cleave = readDoc('cleave.astro');
  const extensions = readDoc('extensions.astro');

  assert.match(index, /typed unavailability if its optional managed service cannot start/);
  assert.match(openspec, /optional managed lifecycle service/);
  assert.match(openspec, /without a direct repository or ledger fallback/);
  assert.match(memory, /optional managed context\/compaction planner/);
  assert.match(memory, /instead of calling a direct planner/);
  assert.match(cleave, /optional managed Git service/);
  assert.match(cleave, /does not rediscover or spawn a direct\s+Git fallback/);
  assert.match(extensions, /failure preserves the previously accepted graph/);
});

test('recovery docs consume canonical maintenance snippets', () => {
  const content = readDoc('recovery.astro');
  const required = [
    'identity',
    'doctor',
    'composition_inspect',
    'contribution_list_project',
    'contribution_disable_dry_run',
    'session_list',
    'session_quarantine_dry_run',
    'resource_list',
    'resource_prune_dry_run',
    'audit_verify',
  ];
  for (const key of required) {
    assert.match(content, new RegExp(`snippet\\("maintenance\\.${key}"\\)`));
  }
  assert.match(content, /snippet\("verify\.release_verify"\)/);
  assert.match(content, /prefers semantic\s+catalog framing/);
  assert.match(content, /Authority events and referenced content blobs are durable truth/);
  assert.match(content, /projector-owned transcript, provider-history, frontend, and compaction outputs are derived/i);
  assert.match(content, /partial_publication/);
  assert.doesNotMatch(content, /omegon-maintain (fix|repair|clean|update|rollback)(\s|<)/);

  const layout = readFileSync(resolve(here, '../src/layouts/Docs.astro'), 'utf8');
  assert.match(layout, /href: "\/docs\/recovery"/);
});

test('sessions docs publish lineage and exactness boundaries', () => {
  const content = readDoc('sessions.astro');
  assert.match(content, /Durable Turn Authority/);
  assert.match(content, /admitted prompts, FIFO queue order/);
  assert.match(content, /Lineage and Resume/);
  assert.match(content, /<strong>Full:<\/strong>/);
  assert.match(content, /<strong>Mixed:<\/strong>/);
  assert.match(content, /<strong>Legacy:<\/strong>/);
  assert.match(content, /Migration is one-way/);
  assert.match(content, /snippet\("slash\.transcript"\)/);
  assert.match(content, /snippet\("slash\.transcript_suffix"\)/);
  assert.match(content, /snippet\("slash\.session_export"\)/);
  assert.match(content, /cancellation request leaves the turn busy/);
  assert.match(content, /Disables interactive semantic session authority and compatibility persistence/);
  assert.match(content, /route\.lease_recorded/);
  assert.match(content, /runtime\/route-leases\.jsonl/);
  assert.match(content, /No current operator command lists historical route leases/);
  assert.doesNotMatch(content, /recent canonical history and summarizes older messages/);
  assert.doesNotMatch(content, /Resume exactly where you left off|full context restored|full replay/i);
});

test('commands and migration docs preserve session compatibility semantics', () => {
  const commands = readDoc('commands.astro');
  const migration = readDoc('migration.astro');
  assert.match(commands, /snippet\("slash\.transcript"\)/);
  assert.match(commands, /snippet\("slash\.transcript_suffix"\)/);
  assert.match(commands, /snippet\("slash\.session_export"\)/);
  assert.match(commands, /not exact transcript authority/);
  assert.match(migration, /explicit full, mixed, or legacy lineage/);
  assert.match(migration, /Migration is one-way|model-facing legacy context once/);
  assert.match(migration, /There is no rollback-to-old-writer mode/);
  assert.doesNotMatch(migration, /recent history plus a summary of older context/);
});

test('provider routing docs preserve contribution and lease boundaries', () => {
  const providers = readDoc('providers.astro');
  const ecosystem = readDoc('ecosystem.astro');
  const faq = readDoc('faq.astro');
  const routing = readDoc('architecture/routing.astro');
  const model = readDoc('three-axis-model.astro');
  const commands = readDoc('commands.astro');
  const install = readDoc('install.astro');
  const migration = readDoc('migration.astro');
  const openspec = readDoc('openspec.astro');
  const publicDocs = docsPages().map(path => readFileSync(path, 'utf8')).join('\n');
  const readme = readFileSync(resolve(here, '../../README.md'), 'utf8');
  const combined = [providers, ecosystem, faq, routing, model, commands, install, migration, openspec, readme].join('\n');

  assert.match(providers, /current provider contribution is non-executable/);
  assert.match(providers, /fallbackProviders/);
  assert.match(providers, /directed and non-transitive/);
  assert.match(providers, /Sessionless callers do not inherit the interactive list/);
  assert.match(providers, /never contains secret material/);
  assert.match(ecosystem, /representative, not an exhaustive inventory of executable providers/);
  assert.match(ecosystem, /Auth\/inventory only; current contribution is non-executable/);
  assert.match(readme, /current provider contribution is non-executable/);
  assert.match(install, /headless execution emits an explicit operator-risk\s+warning and proceeds/);
  assert.match(model, /\/model unpin/);
  assert.match(model, /In the native TUI, <code>\/model route<\/code>/);
  assert.match(commands, /No supported command lists the append-only\s+historical route leases/);
  assert.match(routing, /current dispatch path does not use\s+those diagnostics as an eligibility gate/);
  assert.match(routing, /route\.lease_recorded/);
  assert.match(openspec, /serving route reports/);
  assert.doesNotMatch(combined, /served bridge|selected-vs-served|automatic failover/i);
  assert.doesNotMatch(combined, /stats\.providerCount/);
  assert.doesNotMatch(install, /block headless and automated entry points/i);

  assert.doesNotMatch(publicDocs, /automatic failover/i);
});

test('release verification snippets do not trust arbitrary certificate identities', () => {
  const verification = readFileSync(resolve(snippetsDir, 'verify.yaml'), 'utf8');
  assert.doesNotMatch(verification, /certificate-identity-regexp\s+['"]?\.\*/);
  assert.match(verification, /omegon-maintain --json release verify/);
});

test('package, migration, and release evidence guidance matches the companion contract', () => {
  const install = readDoc('install.astro');
  const migration = readDoc('migration.astro');
  const security = readDoc('security.astro');
  const fixtureReadme = readFileSync(
    resolve(here, '../../core/crates/omegon-maintain/tests/fixtures/README.md'),
    'utf8',
  );

  assert.match(install, /matching <code>omegon<\/code> and <code>omegon-maintain<\/code>/);
  assert.match(install, /resident composition locks for both executables/i);
  assert.match(install, /does not extract or execute optional contributions/);
  assert.match(install, /disabled_tools/);
  assert.match(install, /both <code>--which<\/code> results/);
  assert.match(install, /<code>stale: no<\/code>/);
  assert.match(migration, /Schema-v1 session pairs remain readable as a compatibility\s+import source/);
  assert.match(migration, /There is no rollback-to-old-writer mode/);
  assert.match(security, /Sigstore bundle v0\.3/);
  assert.match(fixtureReadme, /exact archive, canonical package\s+manifest, and Sigstore bundle/);
  assert.doesNotMatch(fixtureReadme, /current release is signed|latest release is signed/i);
});

test('public docs contain no destructive root deletion example', () => {
  for (const page of readdirSync(docsDir).filter(f => f.endsWith('.astro'))) {
    assert.doesNotMatch(readDoc(page), /rm -rf \/ --no-preserve-root/);
  }
});

test('homepage has version selector and install section', () => {
  const content = readFileSync(resolve(here, '../src/pages/index.astro'), 'utf8');

  assert.match(content, /version-select/);
  assert.match(content, /Stable/);
  assert.match(content, /Nightly/);
  assert.match(content, /latestNightly/);
  assert.match(content, /data-tag=\{nightlyTag\}/);
  assert.match(content, /data-url=\{nightlyUrl\}/);
  assert.match(content, /--channel=nightly/);
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

test('dynamic contribution docs preserve trust and lifecycle boundaries', () => {
  const extensions = readDoc('extensions.astro');
  const plugins = readDoc('plugins.astro');
  const security = readDoc('security.astro');
  const combined = `${extensions}\n${plugins}\n${security}`;

  for (const identity of [
    'extension:my-extension',
    'plugin:my-plugin',
    'mcp:project',
    'mcp:acp-client',
  ]) {
    assert.match(combined, new RegExp(identity));
  }
  assert.match(combined, /trustedContributionCode/);
  assert.match(extensions, /terminal quarantine/);
  assert.match(extensions, /best-effort cleanup/);
  assert.match(extensions, /omegon-extension-rs/);
  assert.match(security, /not\s+verified confinement or a security sandbox/);
  assert.doesNotMatch(extensions, /Local paths are symlinked/);
});

test('no page imports siteVariant', () => {
  const pages = readdirSync(docsDir).filter(f => f.endsWith('.astro'));
  for (const page of pages) {
    const content = readDoc(page);
    assert.doesNotMatch(content, /siteVariant/, `${page} still imports siteVariant`);
  }
});

test('snippet parser preserves multiline commands with CRLF input', () => {
  const parsed = parseSnippetYaml([
    'dev_clone_build:',
    '  cmd: |',
    '    git clone https://github.com/styrene-lab/omegon.git',
    '    cd omegon',
    '  desc: "Clone and build"',
    '',
  ].join('\r\n'));

  assert.deepEqual(parsed.dev_clone_build, {
    cmd: 'git clone https://github.com/styrene-lab/omegon.git\ncd omegon',
    desc: 'Clone and build',
  });
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
  const installHtml = readFileSync(resolve(here, '../dist/docs/install/index.html'), 'utf8');
  const recoveryHtml = readFileSync(resolve(here, '../dist/docs/recovery/index.html'), 'utf8');
  const sessionsHtml = readFileSync(resolve(here, '../dist/docs/sessions/index.html'), 'utf8');
  const commandsHtml = readFileSync(resolve(here, '../dist/docs/commands/index.html'), 'utf8');
  const migrationHtml = readFileSync(resolve(here, '../dist/docs/migration/index.html'), 'utf8');
  const securityHtml = readFileSync(resolve(here, '../dist/docs/security/index.html'), 'utf8');
  const rootChangelog = readFileSync(resolve(here, '../../CHANGELOG.md'), 'utf8');

  assert.match(rootChangelog, /^\+\+\+/);
  for (const rendered of [changelogHtml, privacyHtml, termsHtml]) {
    assert.doesNotMatch(rendered, /imported_reference/);
    assert.doesNotMatch(rendered, /\[publication\]/);
    assert.doesNotMatch(rendered, /^\+\+\+/m);
    assert.doesNotMatch(rendered, /^---$/m);
  }
  assert.match(changelogHtml, /local sandbox evidence-substrate smoke suite/);
  assert.match(installHtml, /omegon-maintain --json identity/);
  assert.match(recoveryHtml, /omegon-maintain --json composition inspect/);
  assert.match(recoveryHtml, /contribution disable plugin:formatter --scope project/);
  assert.match(recoveryHtml, /partial_publication/);
  assert.match(sessionsHtml, /\/transcript suffix/);
  assert.match(sessionsHtml, /\/session-export scrollback/);
  assert.match(commandsHtml, /exact committed semantic transcript/);
  assert.match(migrationHtml, /mixed lineage/);
  assert.match(securityHtml, /omegon-maintain --json release verify/);
  for (const rendered of [installHtml, recoveryHtml, securityHtml]) {
    assert.doesNotMatch(rendered, /certificate-identity-regexp[^<]*\.\*/);
  }
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
