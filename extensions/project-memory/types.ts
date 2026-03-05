/**
 * Project Memory — Types
 */

export interface MemoryConfig {
  /** Max lines in active memory before extraction prunes */
  maxLines: number;
  /** Minimum total message tokens before first extraction */
  minimumTokensToInit: number;
  /** Token delta required between extractions */
  minimumTokensBetweenUpdate: number;
  /** Minimum tool calls since last extraction */
  toolCallsBetweenUpdates: number;
  /** Skip auto-extraction if LLM stored >= this many facts since last extraction */
  manualStoreThreshold: number;
  /** Model to use for extraction subagent */
  extractionModel: string;
  /** Timeout for extraction subprocess in ms */
  extractionTimeout: number;
  /** Timeout for shutdown extraction in ms (shorter — blocks exit) */
  shutdownExtractionTimeout: number;
  /** Context % at which degeneracy pressure begins (gradient onset) */
  pressureOnsetPercent: number;
  /** Context % at which to warn the agent to consider compacting */
  compactionWarningPercent: number;
  /** Context % at which to auto-compact without asking */
  compactionAutoPercent: number;
  /** Use local model as fallback when cloud compaction fails */
  compactionLocalFallback: boolean;
  /** Timeout for local model compaction inference in ms */
  compactionLocalTimeout: number;
  /** Max consecutive compaction retry attempts before giving up for the session */
  compactionRetryLimit: number;
  /** Enable Phase 2 global extraction (generalizes project facts to user-level store) */
  globalExtractionEnabled: boolean;
}

export const DEFAULT_CONFIG: MemoryConfig = {
  maxLines: 50,
  minimumTokensToInit: 10_000,
  minimumTokensBetweenUpdate: 5_000,
  toolCallsBetweenUpdates: 8,
  manualStoreThreshold: 3,
  extractionModel: "claude-sonnet-4-6",
  extractionTimeout: 60_000,
  shutdownExtractionTimeout: 15_000,
  pressureOnsetPercent: 55,
  compactionWarningPercent: 75,
  compactionAutoPercent: 85,
  compactionLocalFallback: true,
  compactionLocalTimeout: 120_000,
  compactionRetryLimit: 3,
  globalExtractionEnabled: false,
};

