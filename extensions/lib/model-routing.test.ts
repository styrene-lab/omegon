/**
 * Unit tests for extensions/lib/model-routing.ts
 *
 * Covers all spec scenarios from codex-tier-routing:
 *   - Abstract tier resolves through provider preference
 *   - Resolver skips avoided providers
 *   - Resolver falls back across providers
 *   - Local tier still resolves locally
 *   - Display label mapping
 *   - Default policy shape
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  resolveTier,
  getTierDisplayLabel,
  getDefaultPolicy,
  type RegistryModel,
  type ProviderRoutingPolicy,
} from "./model-routing.ts";

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

function makeModel(provider: string, id: string): RegistryModel {
  return { provider, id };
}

const ANTHROPIC_HAIKU = makeModel("anthropic", "claude-haiku-3-5");
const ANTHROPIC_SONNET = makeModel("anthropic", "claude-sonnet-4-5");
const ANTHROPIC_OPUS = makeModel("anthropic", "claude-opus-4-6");

const OPENAI_HAIKU = makeModel("openai", "gpt-5.1-codex");
const OPENAI_SONNET = makeModel("openai", "gpt-5.3-codex-spark");
const OPENAI_OPUS = makeModel("openai", "gpt-5.4");

const LOCAL_MODEL = makeModel("local", "qwen3:8b");

const ALL_MODELS: RegistryModel[] = [
  ANTHROPIC_HAIKU,
  ANTHROPIC_SONNET,
  ANTHROPIC_OPUS,
  OPENAI_HAIKU,
  OPENAI_SONNET,
  OPENAI_OPUS,
  LOCAL_MODEL,
];

function policy(overrides: Partial<ProviderRoutingPolicy> = {}): ProviderRoutingPolicy {
  return {
    providerOrder: ["anthropic", "openai", "local"],
    avoidProviders: [],
    cheapCloudPreferredOverLocal: false,
    requirePreflightForLargeRuns: true,
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Provider preference order
// ---------------------------------------------------------------------------

describe("resolveTier — provider preference order", () => {
  it("returns openai sonnet when openai is first in order", () => {
    const p = policy({ providerOrder: ["openai", "anthropic", "local"] });
    const result = resolveTier("sonnet", ALL_MODELS, p);
    assert.ok(result, "should resolve");
    assert.equal(result.provider, "openai");
    assert.equal(result.modelId, OPENAI_SONNET.id);
  });

  it("returns anthropic haiku when anthropic is first in order", () => {
    const p = policy({ providerOrder: ["anthropic", "openai", "local"] });
    const result = resolveTier("haiku", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
    assert.equal(result.modelId, ANTHROPIC_HAIKU.id);
  });

  it("returns anthropic opus when anthropic is first in order", () => {
    const p = policy({ providerOrder: ["anthropic", "openai", "local"] });
    const result = resolveTier("opus", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
    assert.equal(result.modelId, ANTHROPIC_OPUS.id);
  });

  it("includes concrete model ID in result", () => {
    const p = policy({ providerOrder: ["openai", "anthropic"] });
    const result = resolveTier("opus", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.modelId, OPENAI_OPUS.id);
    assert.equal(result.tier, "opus");
  });
});

// ---------------------------------------------------------------------------
// Avoid-provider skipping
// ---------------------------------------------------------------------------

describe("resolveTier — avoid-provider skipping", () => {
  it("skips anthropic when avoided and picks openai for opus", () => {
    const p = policy({
      providerOrder: ["anthropic", "openai"],
      avoidProviders: ["anthropic"],
    });
    const result = resolveTier("opus", ALL_MODELS, p);
    assert.ok(result, "should resolve");
    assert.notEqual(result.provider, "anthropic", "must not choose anthropic");
    assert.equal(result.provider, "openai");
    assert.equal(result.modelId, OPENAI_OPUS.id);
  });

  it("skips openai when avoided and picks anthropic for sonnet", () => {
    const p = policy({
      providerOrder: ["openai", "anthropic"],
      avoidProviders: ["openai"],
    });
    const result = resolveTier("sonnet", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
  });

  it("still resolves avoided provider as fallback when it is the only option", () => {
    // Only anthropic models available, but anthropic is avoided
    const anthropicOnly = [ANTHROPIC_OPUS];
    const p = policy({
      providerOrder: ["openai", "anthropic"],
      avoidProviders: ["anthropic"],
    });
    const result = resolveTier("opus", anthropicOnly, p);
    // Fallback should kick in
    assert.ok(result, "should still resolve via fallback");
    assert.equal(result.provider, "anthropic");
  });
});

// ---------------------------------------------------------------------------
// Cross-provider fallback
// ---------------------------------------------------------------------------

describe("resolveTier — cross-provider fallback", () => {
  it("falls back to anthropic haiku when openai lacks haiku-tier models", () => {
    const noOpenAIHaiku = ALL_MODELS.filter((m) => !(m.provider === "openai" && m.id === OPENAI_HAIKU.id));
    const p = policy({ providerOrder: ["openai", "anthropic", "local"] });
    const result = resolveTier("haiku", noOpenAIHaiku, p);
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
    assert.equal(result.modelId, ANTHROPIC_HAIKU.id);
  });

  it("falls back across multiple providers until a match is found", () => {
    const localOnly = [LOCAL_MODEL];
    const p = policy({ providerOrder: ["openai", "anthropic", "local"] });
    // No cloud models for "haiku" — only local
    const result = resolveTier("haiku", localOnly, p);
    // Should end up at "local" provider via fallback
    assert.ok(result, "should resolve via local fallback");
    assert.equal(result.provider, "local");
  });
});

// ---------------------------------------------------------------------------
// Local tier
// ---------------------------------------------------------------------------

describe("resolveTier — local tier", () => {
  it("always resolves local tier to local provider, even when cheap cloud preferred", () => {
    const p = policy({
      providerOrder: ["openai", "anthropic", "local"],
      cheapCloudPreferredOverLocal: true,
    });
    const result = resolveTier("local", ALL_MODELS, p);
    assert.ok(result, "should resolve");
    assert.equal(result.provider, "local");
    assert.equal(result.tier, "local");
  });

  it("does not substitute a cloud model for local tier", () => {
    const p = policy({ providerOrder: ["openai", "anthropic", "local"] });
    const result = resolveTier("local", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.provider, "local");
  });

  it("returns undefined when no local model is registered", () => {
    const noLocal = ALL_MODELS.filter((m) => m.provider !== "local");
    const p = policy();
    const result = resolveTier("local", noLocal, p);
    assert.equal(result, undefined);
  });

  it("picks local model by preference order", () => {
    const locals = [makeModel("local", "devstral:24b"), makeModel("local", "qwen3:8b")];
    const p = policy();
    const result = resolveTier("local", locals, p);
    assert.ok(result);
    assert.equal(result.provider, "local");
    // devstral:24b is higher in PREFERRED_ORDER than qwen3:8b
    assert.equal(result.modelId, "devstral:24b");
  });
});

// ---------------------------------------------------------------------------
// Display label mapping
// ---------------------------------------------------------------------------

describe("getTierDisplayLabel", () => {
  it("maps local → Servitor", () => {
    assert.equal(getTierDisplayLabel("local"), "Servitor");
  });

  it("maps haiku → Adept", () => {
    assert.equal(getTierDisplayLabel("haiku"), "Adept");
  });

  it("maps sonnet → Magos", () => {
    assert.equal(getTierDisplayLabel("sonnet"), "Magos");
  });

  it("maps opus → Archmagos", () => {
    assert.equal(getTierDisplayLabel("opus"), "Archmagos");
  });
});

// ---------------------------------------------------------------------------
// Default policy shape
// ---------------------------------------------------------------------------

describe("getDefaultPolicy", () => {
  it("returns an object with providerOrder array", () => {
    const p = getDefaultPolicy();
    assert.ok(Array.isArray(p.providerOrder));
    assert.ok(p.providerOrder.length > 0);
  });

  it("returns an object with avoidProviders array (empty by default)", () => {
    const p = getDefaultPolicy();
    assert.ok(Array.isArray(p.avoidProviders));
    assert.equal(p.avoidProviders.length, 0);
  });

  it("has cheapCloudPreferredOverLocal flag", () => {
    const p = getDefaultPolicy();
    assert.equal(typeof p.cheapCloudPreferredOverLocal, "boolean");
  });

  it("has requirePreflightForLargeRuns flag", () => {
    const p = getDefaultPolicy();
    assert.equal(typeof p.requirePreflightForLargeRuns, "boolean");
  });

  it("requires preflight for large runs by default", () => {
    const p = getDefaultPolicy();
    assert.equal(p.requirePreflightForLargeRuns, true);
  });
});

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

describe("resolveTier — edge cases", () => {
  it("returns undefined when model list is empty", () => {
    const p = policy();
    const result = resolveTier("sonnet", [], p);
    assert.equal(result, undefined);
  });

  it("returns undefined when no provider in policy has a matching model", () => {
    const p = policy({ providerOrder: ["openai"], avoidProviders: [] });
    const anthropicOnly = [ANTHROPIC_SONNET];
    const result = resolveTier("sonnet", anthropicOnly, p);
    // openai has no model, no fallback available
    assert.equal(result, undefined);
  });

  it("handles duplicate entries in providerOrder without infinite loop", () => {
    const p = policy({ providerOrder: ["anthropic", "anthropic", "openai"] });
    const result = resolveTier("haiku", ALL_MODELS, p);
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
  });

  it("selects highest lexicographic anthropic model within tier prefix", () => {
    // claude-sonnet-4-5 > claude-sonnet-4-0 lexicographically
    const models = [
      makeModel("anthropic", "claude-sonnet-4-0"),
      makeModel("anthropic", "claude-sonnet-4-5"),
    ];
    const p = policy({ providerOrder: ["anthropic"] });
    const result = resolveTier("sonnet", models, p);
    assert.ok(result);
    assert.equal(result.modelId, "claude-sonnet-4-5");
  });
});
