/**
 * Project Memory — Migration
 *
 * Migrates existing markdown-based memory (memory.md, archive/*.md, minds/*)
 * into the SQLite fact store.
 *
 * Migration is idempotent — running it twice won't duplicate facts
 * (dedup via content_hash handles this).
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { FactStore, type StoreFactOptions } from "./factstore.ts";
import { SECTIONS, type SectionName } from "./template.ts";

interface MigrationResult {
  factsImported: number;
  duplicatesSkipped: number;
  archiveFactsImported: number;
  mindsImported: number;
  errors: string[];
}

/**
 * Parse a markdown memory file into section → bullet[] map.
 */
function parseMarkdownMemory(content: string): Map<SectionName, string[]> {
  const sectionBullets = new Map<SectionName, string[]>();
  let currentSection: SectionName | null = null;

  for (const line of content.split("\n")) {
    const sectionMatch = line.match(/^## (.+)$/);
    if (sectionMatch) {
      const name = sectionMatch[1].trim();
      if ((SECTIONS as readonly string[]).includes(name)) {
        currentSection = name as SectionName;
      } else {
        currentSection = null;
      }
      continue;
    }
    if (currentSection && line.trim().startsWith("- ")) {
      if (!sectionBullets.has(currentSection)) {
        sectionBullets.set(currentSection, []);
      }
      // Strip the bullet prefix
      const content = line.trim().replace(/^-\s*/, "").trim();
      if (content) {
        sectionBullets.get(currentSection)!.push(content);
      }
    }
  }

  return sectionBullets;
}

/**
 * Parse an archive file for timestamped sections.
 * Archive files have <!-- Archived YYYY-MM-DD --> markers and
 * optionally [SectionName] prefixes on facts.
 */
function parseArchiveFile(content: string): { date: string; section: SectionName; content: string }[] {
  const facts: { date: string; section: SectionName; content: string }[] = [];
  let currentDate = "";

  for (const line of content.split("\n")) {
    const dateMatch = line.match(/<!--\s*Archived\s+(\d{4}-\d{2}-\d{2})\s*-->/);
    if (dateMatch) {
      currentDate = dateMatch[1];
      continue;
    }

    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("<!--") || trimmed.startsWith("##")) continue;

    // Try to extract [SectionName] prefix
    const sectionMatch = trimmed.match(/^\[([^\]]+)\]\s*(.+)$/);
    let section: SectionName = "Architecture"; // fallback
    let factContent: string;

    if (sectionMatch) {
      const sectionName = sectionMatch[1].trim();
      if ((SECTIONS as readonly string[]).includes(sectionName)) {
        section = sectionName as SectionName;
      }
      factContent = sectionMatch[2].replace(/^-\s*/, "").trim();
    } else {
      factContent = trimmed.replace(/^-\s*/, "").trim();
    }

    if (factContent) {
      facts.push({
        date: currentDate || new Date().toISOString().split("T")[0],
        section,
        content: factContent,
      });
    }
  }

  return facts;
}

/**
 * Migrate an entire .pi/memory directory into a FactStore.
 */
export function migrateToFactStore(memoryDir: string, store: FactStore): MigrationResult {
  const result: MigrationResult = {
    factsImported: 0,
    duplicatesSkipped: 0,
    archiveFactsImported: 0,
    mindsImported: 0,
    errors: [],
  };

  // 1. Migrate default memory.md
  const defaultMemoryPath = path.join(memoryDir, "memory.md");
  if (fs.existsSync(defaultMemoryPath)) {
    try {
      const content = fs.readFileSync(defaultMemoryPath, "utf8");
      const sections = parseMarkdownMemory(content);

      for (const [section, bullets] of sections) {
        for (const bullet of bullets) {
          const { duplicate } = store.storeFact({
            mind: "default",
            section,
            content: bullet,
            source: "migration",
            // Give migrated facts a moderate reinforcement count
            // They've survived in memory, so they're proven durable
            reinforcement_count: 5,
          });
          if (duplicate) {
            result.duplicatesSkipped++;
          } else {
            result.factsImported++;
          }
        }
      }
    } catch (err: any) {
      result.errors.push(`Failed to migrate default memory: ${err.message}`);
    }
  }

  // 2. Migrate archive files
  const archiveDir = path.join(memoryDir, "archive");
  if (fs.existsSync(archiveDir)) {
    try {
      const archiveFiles = fs.readdirSync(archiveDir)
        .filter(f => f.endsWith(".md"))
        .sort();

      for (const file of archiveFiles) {
        const content = fs.readFileSync(path.join(archiveDir, file), "utf8");
        const archiveFacts = parseArchiveFile(content);

        for (const af of archiveFacts) {
          const { duplicate } = store.storeFact({
            mind: "default",
            section: af.section,
            content: af.content,
            source: "migration",
            reinforcement_count: 2, // archived = less durable
          });

          if (!duplicate) {
            // Mark as archived since these were already archived
            // Find the fact we just inserted and archive it
            // Actually, store it as active — let decay handle it naturally
            // since these are older facts they'll have lower confidence
            result.archiveFactsImported++;
          } else {
            result.duplicatesSkipped++;
          }
        }
      }
    } catch (err: any) {
      result.errors.push(`Failed to migrate archive: ${err.message}`);
    }
  }

  // 3. Migrate minds
  const mindsDir = path.join(memoryDir, "minds");
  if (fs.existsSync(mindsDir)) {
    try {
      // Read registry if it exists
      const registryPath = path.join(mindsDir, "registry.json");
      let registry: Record<string, any> = {};
      try {
        registry = JSON.parse(fs.readFileSync(registryPath, "utf8"));
      } catch {
        // No registry — scan directories
      }

      const entries = fs.readdirSync(mindsDir, { withFileTypes: true });
      for (const entry of entries) {
        if (!entry.isDirectory()) continue;
        const mindName = entry.name;

        // Skip state files
        if (mindName === "." || mindName === "..") continue;

        const metaPath = path.join(mindsDir, mindName, "meta.json");
        const mindMemoryPath = path.join(mindsDir, mindName, "memory.md");

        if (!fs.existsSync(mindMemoryPath)) continue;

        try {
          // Read metadata
          let meta: any = {};
          try {
            meta = JSON.parse(fs.readFileSync(metaPath, "utf8"));
          } catch {
            meta = { name: mindName, description: "", status: "active" };
          }

          const regEntry = registry[mindName];

          // Create mind in store if it doesn't exist
          if (!store.mindExists(mindName)) {
            store.createMind(mindName, meta.description ?? "", {
              parent: meta.parent,
              origin_type: regEntry?.origin?.type ?? meta.origin?.type ?? "local",
              origin_path: regEntry?.origin?.path ?? meta.origin?.path,
              readonly: meta.readonly ?? regEntry?.readonly ?? false,
            });
            result.mindsImported++;
          }

          // Migrate facts
          const content = fs.readFileSync(mindMemoryPath, "utf8");
          const sections = parseMarkdownMemory(content);

          for (const [section, bullets] of sections) {
            for (const bullet of bullets) {
              const { duplicate } = store.storeFact({
                mind: mindName,
                section,
                content: bullet,
                source: "migration",
                reinforcement_count: 5,
              });
              if (duplicate) {
                result.duplicatesSkipped++;
              } else {
                result.factsImported++;
              }
            }
          }
        } catch (err: any) {
          result.errors.push(`Failed to migrate mind "${mindName}": ${err.message}`);
        }
      }

      // Migrate active mind state
      const activeStatePath = path.join(mindsDir, "active.json");
      if (fs.existsSync(activeStatePath)) {
        try {
          const state = JSON.parse(fs.readFileSync(activeStatePath, "utf8"));
          if (state.activeMind) {
            store.setActiveMind(state.activeMind);
          }
        } catch {
          // Best effort
        }
      }
    } catch (err: any) {
      result.errors.push(`Failed to migrate minds: ${err.message}`);
    }
  }

  return result;
}

/**
 * Check if migration is needed — does markdown memory exist but no facts.db?
 */
export function needsMigration(memoryDir: string): boolean {
  const hasMarkdown = fs.existsSync(path.join(memoryDir, "memory.md"));
  const hasDb = fs.existsSync(path.join(memoryDir, "facts.db"));
  return hasMarkdown && !hasDb;
}

/**
 * Rename old markdown files after successful migration.
 * Appends .migrated suffix so they're preserved but not re-migrated.
 */
export function markMigrated(memoryDir: string): void {
  const memoryMd = path.join(memoryDir, "memory.md");
  if (fs.existsSync(memoryMd)) {
    fs.renameSync(memoryMd, memoryMd + ".migrated");
  }
}
