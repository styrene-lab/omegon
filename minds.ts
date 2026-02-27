/**
 * Project Memory — Mind Management
 *
 * A "mind" is a named memory store with its own lifecycle:
 *   created → filled → refined → retired/ingested
 *
 * Minds live in .pi/memory/minds/<name>/memory.md
 * The active mind is tracked in .pi/memory/minds/active.json
 */

import * as fs from "node:fs";
import * as path from "node:path";
import { DEFAULT_TEMPLATE, SECTIONS, appendToSection, countContentLines, type SectionName } from "./template.js";

const VALID_MIND_NAME = /^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$/;

function validateMindName(name: string): string {
  if (!VALID_MIND_NAME.test(name)) {
    throw new Error(
      `Invalid mind name "${name}". Names must be 1-64 chars, alphanumeric/dash/underscore, start with alphanumeric.`,
    );
  }
  // Defense in depth — reject anything that resolves outside mindsDir
  if (name.includes("..") || name.includes("/") || name.includes("\\")) {
    throw new Error(`Invalid mind name "${name}". Path traversal characters are not allowed.`);
  }
  return name;
}

export interface MindMeta {
  name: string;
  description: string;
  created: string;
  status: "active" | "refined" | "retired";
  parent?: string; // Name of mind this was ingested from
  lineCount: number;
}

interface ActiveMindState {
  activeMind: string | null; // null = default (legacy .pi/memory/memory.md)
}

export class MindManager {
  private mindsDir: string;
  private stateFile: string;

  constructor(private baseMemoryDir: string) {
    this.mindsDir = path.join(baseMemoryDir, "minds");
    this.stateFile = path.join(this.mindsDir, "active.json");
  }

  init(): void {
    fs.mkdirSync(this.mindsDir, { recursive: true });
  }

  /** Get the name of the currently active mind (null = default) */
  getActiveMindName(): string | null {
    try {
      const state: ActiveMindState = JSON.parse(fs.readFileSync(this.stateFile, "utf8"));
      // Verify the mind still exists
      if (state.activeMind && this.mindExists(state.activeMind)) {
        return state.activeMind;
      }
      return null;
    } catch {
      return null;
    }
  }

  /** Set the active mind */
  setActiveMind(name: string | null): void {
    const state: ActiveMindState = { activeMind: name };
    fs.writeFileSync(this.stateFile, JSON.stringify(state, null, 2), "utf8");
  }

  /** Get the memory directory for a mind */
  getMindDir(name: string): string {
    validateMindName(name);
    return path.join(this.mindsDir, name);
  }

  /** Get the memory.md path for a mind */
  getMindMemoryPath(name: string): string {
    return path.join(this.getMindDir(name), "memory.md");
  }

  /** Get the archive dir for a mind */
  getMindArchiveDir(name: string): string {
    return path.join(this.getMindDir(name), "archive");
  }

  /** Check if a mind exists */
  mindExists(name: string): boolean {
    return fs.existsSync(this.getMindMemoryPath(name));
  }

  /** Create a new mind */
  create(name: string, description: string, template?: string): MindMeta {
    const dir = this.getMindDir(name);
    const archiveDir = this.getMindArchiveDir(name);
    fs.mkdirSync(archiveDir, { recursive: true });

    const content = template ?? DEFAULT_TEMPLATE;
    fs.writeFileSync(this.getMindMemoryPath(name), content, "utf8");

    const meta: MindMeta = {
      name,
      description,
      created: new Date().toISOString().split("T")[0],
      status: "active",
      lineCount: countContentLines(content),
    };
    this.writeMeta(name, meta);
    return meta;
  }

  /** List all minds */
  list(): MindMeta[] {
    try {
      const entries = fs.readdirSync(this.mindsDir, { withFileTypes: true });
      const minds: MindMeta[] = [];

      for (const entry of entries) {
        if (!entry.isDirectory()) continue;

        try {
          const meta = this.readMeta(entry.name);
          if (meta) {
            // Refresh line count
            meta.lineCount = countContentLines(
              this.readMindMemory(entry.name),
            );
            minds.push(meta);
          }
        } catch {
          // Skip invalid mind directories (e.g., .DS_Store, malformed names)
          continue;
        }
      }

      return minds.sort((a, b) => {
        // Active first, then refined, then retired
        const order = { active: 0, refined: 1, retired: 2 };
        return (order[a.status] ?? 3) - (order[b.status] ?? 3);
      });
    } catch {
      return [];
    }
  }

  /** Read a mind's memory content */
  readMindMemory(name: string): string {
    try {
      return fs.readFileSync(this.getMindMemoryPath(name), "utf8");
    } catch {
      return DEFAULT_TEMPLATE;
    }
  }

  /** Write a mind's memory content */
  writeMindMemory(name: string, content: string): void {
    fs.writeFileSync(this.getMindMemoryPath(name), content, "utf8");
  }

  /** Read mind metadata */
  readMeta(name: string): MindMeta | null {
    try {
      const metaPath = path.join(this.getMindDir(name), "meta.json");
      return JSON.parse(fs.readFileSync(metaPath, "utf8"));
    } catch {
      return null;
    }
  }

  /** Write mind metadata */
  writeMeta(name: string, meta: MindMeta): void {
    const metaPath = path.join(this.getMindDir(name), "meta.json");
    fs.writeFileSync(metaPath, JSON.stringify(meta, null, 2), "utf8");
  }

  /** Update a mind's status */
  setStatus(name: string, status: MindMeta["status"]): void {
    const meta = this.readMeta(name);
    if (meta) {
      meta.status = status;
      this.writeMeta(name, meta);
    }
  }

  /**
   * Ingest a mind into another (merge memories, retire source).
   * Preserves section structure — bullets are appended under their
   * respective section headers in the target.
   */
  ingest(sourceName: string, targetName: string): { factsIngested: number } {
    if (sourceName === targetName) {
      throw new Error(`Cannot ingest mind "${sourceName}" into itself`);
    }

    const sourceContent = this.readMindMemory(sourceName);
    let targetContent = this.readMindMemory(targetName);

    // Parse source into section → bullet[] map
    const sectionBullets = this.parseSectionBullets(sourceContent);

    // Append bullets into target under their respective sections
    let totalIngested = 0;
    for (const [section, bullets] of sectionBullets) {
      for (const bullet of bullets) {
        const updated = appendToSection(targetContent, section, bullet);
        if (updated !== targetContent) {
          targetContent = updated;
          totalIngested++;
        }
      }
    }

    this.writeMindMemory(targetName, targetContent);
    this.setStatus(sourceName, "retired");

    const targetMeta = this.readMeta(targetName);
    if (targetMeta) {
      targetMeta.lineCount = countContentLines(targetContent);
      this.writeMeta(targetName, targetMeta);
    }

    return { factsIngested: totalIngested };
  }

  /** Ingest a mind into the default project memory */
  ingestIntoDefault(sourceName: string): { factsIngested: number } {
    const sourceContent = this.readMindMemory(sourceName);
    const defaultMemoryPath = path.join(this.baseMemoryDir, "memory.md");
    let targetContent: string;
    try {
      targetContent = fs.readFileSync(defaultMemoryPath, "utf8");
    } catch {
      targetContent = DEFAULT_TEMPLATE;
    }

    // Parse source and append section-aware
    const sectionBullets = this.parseSectionBullets(sourceContent);
    let totalIngested = 0;

    for (const [section, bullets] of sectionBullets) {
      for (const bullet of bullets) {
        const updated = appendToSection(targetContent, section, bullet);
        if (updated !== targetContent) {
          targetContent = updated;
          totalIngested++;
        }
      }
    }

    fs.writeFileSync(defaultMemoryPath, targetContent, "utf8");
    this.setStatus(sourceName, "retired");
    return { factsIngested: totalIngested };
  }

  /** Delete a mind entirely */
  delete(name: string): void {
    const dir = this.getMindDir(name);
    fs.rmSync(dir, { recursive: true, force: true });

    // If this was the active mind, reset to default
    if (this.getActiveMindName() === name) {
      this.setActiveMind(null);
    }
  }

  /** Fork a mind (create a copy with a new name) */
  fork(sourceName: string, newName: string, description: string): MindMeta {
    const content = this.readMindMemory(sourceName);
    const meta = this.create(newName, description, content);
    meta.parent = sourceName;
    this.writeMeta(newName, meta);
    return meta;
  }

  /**
   * Parse content into sections and their associated bullets.
   * Returns a map of section names to arrays of bullet lines.
   */
  private parseSectionBullets(content: string): Map<SectionName, string[]> {
    const sectionBullets = new Map<SectionName, string[]>();
    let currentSection: SectionName | null = null;

    for (const line of content.split("\n")) {
      const sectionMatch = line.match(/^## (.+)$/);
      if (sectionMatch) {
        const sectionName = sectionMatch[1].trim();
        if ((SECTIONS as readonly string[]).includes(sectionName)) {
          currentSection = sectionName as SectionName;
        } else {
          currentSection = null;
        }
        continue;
      }
      if (currentSection && line.trim().startsWith("- ")) {
        if (!sectionBullets.has(currentSection)) {
          sectionBullets.set(currentSection, []);
        }
        sectionBullets.get(currentSection)!.push(line);
      }
    }

    return sectionBullets;
  }
}
