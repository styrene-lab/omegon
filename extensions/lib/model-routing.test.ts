import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  classifyTransientFailure,
  classifyUpstreamFailure,
  clampThinkingLevel,
  getDefaultCapabilityProfile,
  getDefaultPolicy,
  getRoleDisplayLabel,
  getTierDisplayLabel,
  resolveCapabilityRole,
  resolveTier,
  TRANSIENT_PROVIDER_COOLDOWN_MS,
  type CapabilityCandidate,
  type CapabilityProfile,
  type ProviderRoutingPolicy,
  type RegistryModel,
  withCandidateCooldown,
  withProviderCooldown,
} from "./model-routing.ts";
import { sharedState } from "./shared-state.ts";

function makeModel(provider: string, id: string): RegistryModel {
  return { provider, id };
}

function makeCandidate(
  id: string,
  provider: CapabilityCandidate["provider"],
  source: CapabilityCandidate["source"],
  weight: CapabilityCandidate["weight"],
  maxThinking: CapabilityCandidate["maxThinking"],
): CapabilityCandidate {
  return { id, provider, source, weight, maxThinking };
}

const ANTHROPIC_HAIKU = makeModel("anthropic", "claude-haiku-3-5");
const ANTHROPIC_SONNET = makeModel("anthropic", "claude-sonnet-4-5");
const ANTHROPIC_OPUS = makeModel("anthropic", "claude-opus-4-6");
const OPENAI_HAIKU = makeModel("openai", "gpt-5.1-codex");
const OPENAI_SONNET = makeModel("openai", "gpt-5.3-codex-spark");
const OPENAI_OPUS = makeModel("openai", "gpt-5.4");
const LOCAL_LIGHT = makeModel("local", "qwen3:4b");
const LOCAL_HEAVY = makeModel("local", "devstral:24b");

const ALL_MODELS: RegistryModel[] = [
  ANTHROPIC_HAIKU,
  ANTHROPIC_SONNET,
  ANTHROPIC_OPUS,
  OPENAI_HAIKU,
  OPENAI_SONNET,
  OPENAI_OPUS,
  LOCAL_LIGHT,
  LOCAL_HEAVY,
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

function profile(overrides: Partial<CapabilityProfile> = {}): CapabilityProfile {
  return {
    ...getDefaultCapabilityProfile(ALL_MODELS),
    ...overrides,
  };
}

describe("resolveTier — compatibility tier routing", () => {
  it("returns openai victory when openai is first in order", () => {
    const result = resolveTier("victory", ALL_MODELS, policy({ providerOrder: ["openai", "anthropic", "local"] }));
    assert.ok(result);
    assert.equal(result.provider, "openai");
    assert.equal(result.modelId, OPENAI_SONNET.id);
    assert.equal(result.maxThinking, "medium");
  });

  it("returns anthropic gloriana when anthropic is first in order", () => {
    const result = resolveTier("gloriana", ALL_MODELS, policy());
    assert.ok(result);
    assert.equal(result.provider, "anthropic");
    assert.equal(result.modelId, ANTHROPIC_OPUS.id);
    assert.equal(result.maxThinking, "high");
  });

  it("returns undefined when policy would require confirmation for cross-source fallback", () => {
    const upstreamOnly = [OPENAI_HAIKU, LOCAL_HEAVY];
    const customProfile = profile({
      roles: {
        ...getDefaultCapabilityProfile(upstreamOnly).roles,
        adept: {
          candidates: [
            makeCandidate(OPENAI_HAIKU.id, "openai", "upstream", "light", "low"),
            makeCandidate(LOCAL_HEAVY.id, "local", "local", "heavy", "medium"),
          ],
        },
      },
    });
    const now = Date.now();
    const runtimeState = withProviderCooldown(undefined, "openai", "429", now);
    const result = resolveTier("retribution", upstreamOnly, policy({ providerOrder: ["openai", "local"] }), runtimeState, customProfile);
    assert.equal(result, undefined);
  });

  it("always resolves local tier to local provider", () => {
    const result = resolveTier("local", ALL_MODELS, policy({ cheapCloudPreferredOverLocal: true }));
    assert.ok(result);
    assert.equal(result.provider, "local");
  });
});

describe("resolveCapabilityRole — fallback semantics", () => {
  it("same-role cross-provider fallback happens automatically when allowed", () => {
    const models = [ANTHROPIC_SONNET, OPENAI_SONNET];
    const customProfile: CapabilityProfile = {
      roles: {
        archmagos: { candidates: [] },
        magos: {
          candidates: [
            makeCandidate(ANTHROPIC_SONNET.id, "anthropic", "upstream", "normal", "high"),
            makeCandidate(OPENAI_SONNET.id, "openai", "upstream", "normal", "medium"),
          ],
        },
        adept: { candidates: [] },
        servitor: { candidates: [] },
        servoskull: { candidates: [] },
      },
      internalAliases: {},
      policy: {
        sameRoleCrossProvider: "allow",
        crossSource: "ask",
        heavyLocal: "ask",
        unknownLocalPerformance: "ask",
      },
    };
    const runtimeState = withProviderCooldown(undefined, "anthropic", "429", 1000);
    const result = resolveCapabilityRole("magos", models, policy(), customProfile, runtimeState, 1001);
    assert.equal(result.ok, true);
    assert.equal(result.selected?.candidate.provider, "openai");
    assert.equal(result.selected?.candidate.id, OPENAI_SONNET.id);
  });

  it("cross-source fallback requires operator approval when policy asks", () => {
    const models = [OPENAI_HAIKU, LOCAL_HEAVY];
    const customProfile: CapabilityProfile = {
      roles: {
        archmagos: { candidates: [] },
        magos: { candidates: [] },
        adept: {
          candidates: [
            makeCandidate(OPENAI_HAIKU.id, "openai", "upstream", "light", "low"),
            makeCandidate(LOCAL_HEAVY.id, "local", "local", "heavy", "medium"),
          ],
        },
        servitor: { candidates: [] },
        servoskull: { candidates: [] },
      },
      internalAliases: { retribution: "adept" },
      policy: {
        sameRoleCrossProvider: "allow",
        crossSource: "ask",
        heavyLocal: "ask",
        unknownLocalPerformance: "ask",
      },
    };
    const runtimeState = withProviderCooldown(undefined, "openai", "session limit", 1000);
    const result = resolveCapabilityRole("adept", models, policy({ providerOrder: ["openai", "local"] }), customProfile, runtimeState, 1001);
    assert.equal(result.ok, false);
    assert.equal(result.requiresConfirmation, true);
    assert.equal(result.blockedBy, "cross-source");
    assert.match(result.reason ?? "", /requires operator confirmation/i);
  });

  it("servoskull candidates preserve thinking-off ceilings", () => {
    const customProfile = getDefaultCapabilityProfile([OPENAI_HAIKU, LOCAL_LIGHT]);
    const result = resolveCapabilityRole("servoskull", [OPENAI_HAIKU, LOCAL_LIGHT], policy(), customProfile);
    assert.equal(result.ok, true);
    assert.equal(result.selected?.candidate.maxThinking, "off");
  });

  it("supports overlapping tiers with different thinking ceilings", () => {
    const sameModel = makeModel("openai", "gpt-4.1");
    const customProfile: CapabilityProfile = {
      roles: {
        archmagos: { candidates: [makeCandidate(sameModel.id, "openai", "upstream", "normal", "high")] },
        magos: { candidates: [makeCandidate(sameModel.id, "openai", "upstream", "normal", "medium")] },
        adept: { candidates: [] },
        servitor: { candidates: [] },
        servoskull: { candidates: [] },
      },
      internalAliases: {},
      policy: {
        sameRoleCrossProvider: "allow",
        crossSource: "ask",
        heavyLocal: "ask",
        unknownLocalPerformance: "ask",
      },
    };
    const archmagos = resolveCapabilityRole("archmagos", [sameModel], policy(), customProfile);
    const magos = resolveCapabilityRole("magos", [sameModel], policy(), customProfile);
    assert.equal(archmagos.selected?.candidate.id, sameModel.id);
    assert.equal(magos.selected?.candidate.id, sameModel.id);
    assert.equal(archmagos.selected?.candidate.maxThinking, "high");
    assert.equal(magos.selected?.candidate.maxThinking, "medium");
  });
});

describe("cooldown helpers", () => {
  it("candidate cooldown blocks resolution until expiry", () => {
    const candidate = makeCandidate(ANTHROPIC_SONNET.id, "anthropic", "upstream", "normal", "high");
    const runtimeState = withCandidateCooldown(undefined, candidate, "429", 1000, 5000);
    const customProfile: CapabilityProfile = {
      roles: {
        archmagos: { candidates: [] },
        magos: { candidates: [candidate] },
        adept: { candidates: [] },
        servitor: { candidates: [] },
        servoskull: { candidates: [] },
      },
      internalAliases: {},
      policy: {
        sameRoleCrossProvider: "allow",
        crossSource: "ask",
        heavyLocal: "ask",
        unknownLocalPerformance: "ask",
      },
    };
    const blocked = resolveCapabilityRole("magos", [ANTHROPIC_SONNET], policy(), customProfile, runtimeState, 1500);
    assert.equal(blocked.ok, false);
    const availableAgain = resolveCapabilityRole("magos", [ANTHROPIC_SONNET], policy(), customProfile, runtimeState, 7000);
    assert.equal(availableAgain.ok, true);
  });

  it("provider cooldown uses fixed five-minute default", () => {
    const runtimeState = withProviderCooldown(undefined, "anthropic", "429", 1000);
    assert.equal(runtimeState.providerCooldowns?.anthropic?.until, 1000 + TRANSIENT_PROVIDER_COOLDOWN_MS);
  });
});

describe("upstream failure classification", () => {
  it("classifies retryable flakes separately from rate limits", () => {
    const flake = classifyUpstreamFailure(new Error("server_error from upstream"));
    const rateLimit = classifyUpstreamFailure(new Error("429 rate limit exceeded"));

    assert.equal(flake.class, "retryable-flake");
    assert.equal(flake.recoveryAction, "retry-same-model");
    assert.equal(flake.retryable, true);
    assert.equal(flake.cooldownProvider, false);

    assert.equal(rateLimit.class, "rate-limit");
    assert.equal(rateLimit.recoveryAction, "failover");
    assert.equal(rateLimit.retryable, false);
    assert.equal(rateLimit.cooldownProvider, true);
    assert.equal(rateLimit.cooldownCandidate, true);
  });

  it("classifies explicit backoff as failover rather than same-model retry", () => {
    const classification = classifyUpstreamFailure(new Error("provider says try again later"));
    assert.equal(classification.class, "backoff");
    assert.equal(classification.recoveryAction, "failover");
    assert.equal(classification.retryable, false);
  });

  it("classifies Codex JSON server_error payloads as retryable flakes", () => {
    const classification = classifyUpstreamFailure(new Error('Codex error: {"type":"error","error":{"type":"server_error","code":"server_error","message":"An error occurred while processing your request."}}'));
    assert.equal(classification.class, "retryable-flake");
    assert.equal(classification.recoveryAction, "retry-same-model");
    assert.equal(classification.retryable, true);
  });

  it("classifies oversized image API errors as invalid-request", () => {
    const exact = classifyUpstreamFailure(new Error(
      'Error: 400 {"type":"error","error":{"type":"invalid_request_error","message":"messages.7.content.116.image.source.base64.data: At least one of the image dimensions exceed max allowed size: 8000 pixels"},"request_id":"req_011CYuajxFckkfCkUNKaaeMY"}'
    ));
    assert.equal(exact.class, "invalid-request");
    assert.equal(exact.retryable, false);
    assert.equal(exact.recoveryAction, "surface");
    assert.ok(exact.summary.includes("image"));

    // Simpler variant
    const simple = classifyUpstreamFailure(new Error("image dimensions exceed max allowed size: 8000 pixels"));
    assert.equal(simple.class, "invalid-request");
  });

  it("classifies generic invalid_request_error as invalid-request", () => {
    const classification = classifyUpstreamFailure(new Error('{"type":"error","error":{"type":"invalid_request_error","message":"some other validation issue"}}'));
    assert.equal(classification.class, "invalid-request");
    assert.equal(classification.retryable, false);
  });

  it("keeps auth, quota, tool-output, and context overflow out of generic transient retry", () => {
    assert.equal(classifyUpstreamFailure(new Error("invalid api key")).class, "auth");
    assert.equal(classifyUpstreamFailure(new Error("insufficient_quota")).class, "quota");
    assert.equal(classifyUpstreamFailure(new Error("malformed tool output from helper")).class, "tool-output");
    assert.equal(classifyUpstreamFailure(new Error("schema validation failed: malformed json")).class, "tool-output");
    assert.equal(classifyUpstreamFailure(new Error("maximum context length exceeded")).class, "context-overflow");

    assert.equal(classifyTransientFailure(new Error("invalid api key")), false);
    assert.equal(classifyTransientFailure(new Error("maximum context length exceeded")), false);
  });

  it("classifies user-initiated aborts as user-abort with handled-elsewhere recovery", () => {
    const cases = [
      "Operation aborted",
      "Command aborted",
      "user aborted",
      "AbortedError",
      "request aborted by client",
      "abort was called",
      "aborted",
    ];
    for (const msg of cases) {
      const c = classifyUpstreamFailure(new Error(msg));
      assert.equal(c.class, "user-abort", `Expected user-abort for: ${msg}`);
      assert.equal(c.recoveryAction, "handled-elsewhere", `Expected handled-elsewhere for: ${msg}`);
      assert.equal(c.retryable, false, `Expected non-retryable for: ${msg}`);
    }
    // user-abort must never be treated as a transient (retryable) failure
    assert.equal(classifyTransientFailure(new Error("Operation aborted")), false);
    assert.equal(classifyTransientFailure(new Error("Command aborted")), false);
  });
});

describe("display labels and defaults", () => {
  it("maps tiers to operator-facing labels", () => {
    assert.equal(getTierDisplayLabel("local"), "Servitor [Local]");
    assert.equal(getTierDisplayLabel("retribution"), "Adept [Retribution Class]");
    assert.equal(getTierDisplayLabel("victory"), "Magos [Victory Class]");
    assert.equal(getTierDisplayLabel("gloriana"), "Archmagos [Gloriana Class]");
  });

  it("maps public roles to labels", () => {
    assert.equal(getRoleDisplayLabel("servoskull"), "Servoskull");
  });

  it("default profile includes the full public role ladder", () => {
    const p = getDefaultCapabilityProfile(ALL_MODELS);
    assert.deepEqual(Object.keys(p.roles), ["archmagos", "magos", "adept", "servitor", "servoskull"]);
    assert.equal(p.policy.crossSource, "ask");
    assert.equal(p.policy.heavyLocal, "ask");
  });

  it("default policy shape remains stable", () => {
    const p = getDefaultPolicy();
    assert.ok(Array.isArray(p.providerOrder));
    assert.ok(Array.isArray(p.avoidProviders));
    assert.equal(typeof p.cheapCloudPreferredOverLocal, "boolean");
    assert.equal(typeof p.requirePreflightForLargeRuns, "boolean");
  });
});

describe("thinking ceiling helpers", () => {
  it("clamps requested thinking to candidate ceiling", () => {
    assert.equal(clampThinkingLevel("high", "medium"), "medium");
    assert.equal(clampThinkingLevel("low", "medium"), "low");
  });
});

describe("sharedState.routingPolicy", () => {
  it("is initialized with a default policy on first import", () => {
    assert.ok(sharedState.routingPolicy);
    assert.ok(Array.isArray(sharedState.routingPolicy.providerOrder));
    assert.ok(Array.isArray(sharedState.routingPolicy.avoidProviders));
  });

  it("records temporary avoidProviders state", () => {
    sharedState.routingPolicy = {
      ...getDefaultPolicy(),
      avoidProviders: ["anthropic"],
      notes: "Anthropic budget is low today",
    };
    assert.ok(sharedState.routingPolicy.avoidProviders.includes("anthropic"));
    assert.match(sharedState.routingPolicy.notes ?? "", /low/i);
  });
});
