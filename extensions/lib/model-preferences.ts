import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export interface PersistedDriverSelection {
  provider: string;
  modelId: string;
}

export interface PiConfig {
  lastUsedModel?: PersistedDriverSelection;
  operatorProfile?: unknown;
  [key: string]: unknown;
}

function configPath(root: string): string {
  return join(root, ".pi", "config.json");
}

export function loadPiConfig(root: string): PiConfig {
  try {
    const path = configPath(root);
    if (!existsSync(path)) return {};
    const raw = readFileSync(path, "utf-8");
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed) ? parsed as PiConfig : {};
  } catch {
    return {};
  }
}

export function savePiConfig(root: string, config: PiConfig): void {
  const dir = join(root, ".pi");
  mkdirSync(dir, { recursive: true });
  writeFileSync(configPath(root), JSON.stringify(config, null, 2) + "\n", "utf-8");
}

export function readLastUsedModel(root: string): PersistedDriverSelection | undefined {
  const value = loadPiConfig(root).lastUsedModel;
  if (!value || typeof value !== "object") return undefined;
  const record = value as unknown as Record<string, unknown>;
  const provider = record.provider;
  const modelId = record.modelId;
  if (typeof provider !== "string" || typeof modelId !== "string") return undefined;
  return { provider, modelId };
}

export function writeLastUsedModel(root: string, selection: PersistedDriverSelection): void {
  const config = loadPiConfig(root);
  config.lastUsedModel = selection;
  savePiConfig(root, config);
}
