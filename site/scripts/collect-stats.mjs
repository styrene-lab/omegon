#!/usr/bin/env node
// Derives hard project stats from source-of-truth files and the GitHub API.
// Writes site/src/data/stats.json for Astro to import at build time.
//
// Sources:
//   - Cargo.toml workspace members → crate count
//   - GitHub release assets → binary size (compressed + uncompressed)
//   - core/crates/omegon/src/tools/ → tool file count (lower bound)
//   - auth.rs PROVIDERS array → provider count + names (canonical source)
//   - skills/ directory → skill count
//   - tools/web_search.rs → search provider count

import { readFileSync, writeFileSync, readdirSync, mkdirSync, existsSync } from "fs";
import { resolve, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "../..");
const OUT = resolve(__dirname, "../src/data/stats.json");
const REPO = "styrene-lab/omegon";

// ── Crate count from Cargo.toml ──────────────────────────────────────────────

function getCrateCount() {
  const cargo = readFileSync(resolve(ROOT, "Cargo.toml"), "utf-8");
  const membersBlock = cargo.match(/members\s*=\s*\[([\s\S]*?)\]/);
  if (!membersBlock) return null;
  const paths = membersBlock[1].match(/"[^"]+"/g);
  return paths ? paths.length : null;
}

// ── Binary size from GitHub release assets ───────────────────────────────────

async function getBinarySize() {
  const releaseTag = process.env.OMEGON_SITE_RELEASE_TAG || null;
  const headers = { "User-Agent": "omegon-site-build" };
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;
  if (token) headers["Authorization"] = `token ${token}`;

  try {
    const res = await fetch(
      releaseTag
        ? `https://api.github.com/repos/${REPO}/releases/tags/${encodeURIComponent(releaseTag)}`
        : `https://api.github.com/repos/${REPO}/releases/latest`,
      { headers },
    );
    if (!res.ok) throw new Error(`${res.status}`);
    const data = await res.json();

    // Pick the macOS arm64 tarball as the reference asset
    const asset = data.assets.find((a) =>
      /aarch64.*apple.*darwin.*\.tar\.gz$/.test(a.name),
    );
    if (!asset) return null;

    const downloadMB = Math.round(asset.size / 1048576);

    return { downloadMB, tag: data.tag_name };
  } catch {
    return null;
  }
}

// ── Provider info from auth.rs (canonical source) ───────────────────────────
// Parses the PROVIDERS static array in auth.rs to extract provider IDs and
// display names. This is the single source of truth — no hardcoded list.

function getProviderInfo() {
  try {
    const src = readFileSync(
      resolve(ROOT, "core/crates/omegon/src/auth.rs"),
      "utf-8",
    );
    // Extract the PROVIDERS block
    const block = src.match(/pub static PROVIDERS[\s\S]*?\n\];/);
    if (!block) return null;

    // Split at the "Non-inference services" comment — everything before
    // it is an inference provider, everything after is not.
    const parts = block[0].split(/\/\/.*Non-inference services/);
    const inferenceBlock = parts[0] || block[0];

    // Parse each ProviderCredential entry from the inference section only
    const entries = [...inferenceBlock.matchAll(/id:\s*"([^"]+)"[\s\S]*?display_name:\s*"([^"]+)"/g)];
    const inferenceProviders = entries.map(([, id, name]) => ({ id, name }));

    return {
      count: inferenceProviders.length,
      names: inferenceProviders.map((p) => p.name),
      ids: inferenceProviders.map((p) => p.id),
    };
  } catch {
    return null;
  }
}

// ── Tool count from tools directory ──────────────────────────────────────────

function getToolFileCount() {
  try {
    const toolsDir = resolve(ROOT, "core/crates/omegon/src/tools");
    const files = readdirSync(toolsDir).filter(
      (f) => f.endsWith(".rs") && f !== "mod.rs",
    );
    return files.length;
  } catch {}
  return null;
}

// ── Skill count from skills directory ────────────────────────────────────────

function getSkillCount() {
  try {
    const skillsDir = resolve(ROOT, "skills");
    if (!existsSync(skillsDir)) return null;
    const dirs = readdirSync(skillsDir, { withFileTypes: true })
      .filter((d) => d.isDirectory());
    return dirs.length;
  } catch {}
  return null;
}

// ── Search provider count from web_search.rs ────────────────────────────────

function getSearchProviderCount() {
  try {
    const src = readFileSync(
      resolve(ROOT, "core/crates/omegon/src/tools/web_search.rs"),
      "utf-8",
    );
    // Count distinct search provider branches in the provider match
    const matches = src.matchAll(/"(brave|tavily|google|bing|duckduckgo|searxng|serper|exa)"/g);
    const providers = new Set();
    for (const m of matches) {
      providers.add(m[1]);
    }
    return providers.size || null;
  } catch {}
  return null;
}

// ── Main ─────────────────────────────────────────────────────────────────────

async function main() {
  const crateCount = getCrateCount();
  const binaryInfo = await getBinarySize();
  const providerInfo = getProviderInfo();
  const toolFiles = getToolFileCount();
  const skillCount = getSkillCount();
  const searchProviderCount = getSearchProviderCount();

  const stats = {
    crateCount,
    downloadMB: binaryInfo?.downloadMB ?? null,
    releaseTag: binaryInfo?.tag ?? process.env.OMEGON_SITE_RELEASE_TAG ?? null,
    providerCount: providerInfo?.count ?? null,
    providerNames: providerInfo?.names ?? [],
    providerIds: providerInfo?.ids ?? [],
    toolFiles,
    skillCount,
    searchProviderCount,
    collectedAt: new Date().toISOString(),
  };

  mkdirSync(dirname(OUT), { recursive: true });
  writeFileSync(OUT, JSON.stringify(stats, null, 2) + "\n");

  console.log(`[collect-stats] Stats collected:`);
  console.log(`  crates: ${stats.crateCount}`);
  console.log(`  download: ~${stats.downloadMB}MB (${stats.releaseTag})`);
  console.log(`  providers: ${stats.providerCount} (${stats.providerNames.join(", ")})`);
  console.log(`  tool files: ${stats.toolFiles}`);
  console.log(`  skills: ${stats.skillCount}`);
  console.log(`  search providers: ${stats.searchProviderCount}`);
  console.log(`[collect-stats] Wrote ${OUT}`);
}

main();
