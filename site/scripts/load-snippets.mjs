// Loads all YAML snippet files from site/snippets/ and writes a merged JSON
// file to site/src/data/snippets.json for Astro to import at build time.
//
// Each snippet is keyed by "filename.key" (e.g. "install.quick_install").
// Uses a minimal YAML parser — only supports the flat key/cmd/desc structure
// used in our snippet files. No external dependencies.

import { readdirSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const SNIPPETS_DIR = resolve(__dirname, "../snippets");
const OUT = resolve(__dirname, "../src/data/snippets.json");

// Minimal YAML parser for our flat snippet format:
//   key:
//     cmd: "value" OR cmd: | (multiline block)
//     desc: "value"
export function parseSnippetYaml(text) {
  const result = {};
  let currentKey = null;
  let currentField = null;
  let blockIndent = null;
  let blockLines = [];

  function flushBlock() {
    if (currentKey && currentField && blockLines.length > 0) {
      if (!result[currentKey]) result[currentKey] = {};
      result[currentKey][currentField] = blockLines.join("\n");
    }
    blockLines = [];
    blockIndent = null;
    currentField = null;
  }

  for (const line of text.replaceAll("\r\n", "\n").replaceAll("\r", "\n").split("\n")) {
    // Skip comments and blank lines at top level
    if (line.match(/^\s*#/) || line.match(/^\s*$/)) {
      if (blockIndent !== null && line.match(/^\s*$/)) {
        // blank line inside a block — could be part of multiline
        continue;
      }
      continue;
    }

    // Top-level key (no leading whitespace, ends with colon)
    const topMatch = line.match(/^([a-zA-Z_][a-zA-Z0-9_]*):\s*$/);
    if (topMatch) {
      flushBlock();
      currentKey = topMatch[1];
      if (!result[currentKey]) result[currentKey] = {};
      continue;
    }

    // Field with inline value: "  cmd: value" or "  desc: value"
    const fieldMatch = line.match(/^\s{2,}(cmd|desc):\s*(.+)$/);
    if (fieldMatch && currentKey) {
      flushBlock();
      const [, field, rawVal] = fieldMatch;

      // Check for block scalar indicator
      if (rawVal.trim() === "|") {
        currentField = field;
        blockIndent = null;
        blockLines = [];
        continue;
      }

      // Strip surrounding quotes if present
      let val = rawVal.trim();
      if ((val.startsWith('"') && val.endsWith('"')) ||
          (val.startsWith("'") && val.endsWith("'"))) {
        val = val.slice(1, -1);
      }
      result[currentKey][field] = val;
      continue;
    }

    // Block scalar continuation line
    if (blockIndent === null && currentField && line.match(/^\s+\S/)) {
      blockIndent = line.match(/^(\s+)/)[1].length;
    }
    if (blockIndent !== null && (line.startsWith(" ".repeat(blockIndent)) || line.trim() === "")) {
      blockLines.push(line.slice(blockIndent));
      continue;
    }
  }
  flushBlock();
  return result;
}

const merged = {};
let total = 0;
const snippetFiles = readdirSync(SNIPPETS_DIR)
  .filter((file) => file.endsWith(".yaml") || file.endsWith(".yml"))
  .sort((a, b) => a.localeCompare(b, "en"));

for (const file of snippetFiles) {
  const category = basename(file, file.endsWith(".yaml") ? ".yaml" : ".yml");
  const raw = readFileSync(resolve(SNIPPETS_DIR, file), "utf-8");
  const data = parseSnippetYaml(raw);

  for (const [key, entry] of Object.entries(data)) {
    if (!entry || !entry.cmd) continue;
    const fullKey = `${category}.${key}`;
    merged[fullKey] = {
      cmd: entry.cmd.trimEnd(),
      desc: entry.desc || "",
    };
    total++;
  }
}

const requiredKeys = ["cli.dev_clone_build"];
const missingKeys = requiredKeys.filter((key) => !merged[key]?.cmd);
if (missingKeys.length > 0) {
  throw new Error(`Missing required snippet key(s): ${missingKeys.join(", ")}`);
}

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, JSON.stringify(merged, null, 2) + "\n");
console.log(`[load-snippets] ${total} snippets from ${SNIPPETS_DIR} → ${OUT}`);
