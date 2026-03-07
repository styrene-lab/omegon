/**
 * Extraction trigger logic — determines when background extraction should run.
 */

import type { MemoryConfig } from "./types.ts";

export interface ExtractionTriggerState {
  lastExtractedTokens: number;
  toolCallsSinceExtract: number;
  manualStoresSinceExtract: number;
  isInitialized: boolean;
  isRunning: boolean;
}

export function createTriggerState(): ExtractionTriggerState {
  return {
    lastExtractedTokens: 0,
    toolCallsSinceExtract: 0,
    manualStoresSinceExtract: 0,
    isInitialized: false,
    isRunning: false,
  };
}

export function shouldExtract(
  state: ExtractionTriggerState,
  currentTokens: number,
  config: MemoryConfig,
  consecutiveFailures: number = 0,
): boolean {
  if (state.isRunning) return false;

  // Exponential backoff on consecutive failures: skip 2^n extraction opportunities
  // (1 skip after 1 failure, 2 after 2, 4 after 3, cap at 16)
  if (consecutiveFailures > 0) {
    const backoffSlots = Math.min(1 << consecutiveFailures, 16);
    if (state.toolCallsSinceExtract % backoffSlots !== 0) return false;
  }

  // Only suppress for manual stores after first extraction has established baseline.
  if (state.isInitialized && state.manualStoresSinceExtract >= config.manualStoreThreshold) {
    return false;
  }

  const tokenDelta = currentTokens - state.lastExtractedTokens;

  if (!state.isInitialized) {
    return currentTokens >= config.minimumTokensToInit;
  }

  return tokenDelta >= config.minimumTokensBetweenUpdate && state.toolCallsSinceExtract >= config.toolCallsBetweenUpdates;
}
