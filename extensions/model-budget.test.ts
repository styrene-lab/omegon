import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  buildRecoveryEvent,
  buildSetModelTierDescription,
  buildTierCommandDescription,
  classifyRecoveryFailure,
} from "./model-budget.ts";
import { clampThinkingLevel } from "./lib/model-routing.ts";

describe("model-budget copy", () => {
  it("describes set_model_tier as provider-aware", () => {
    const description = buildSetModelTierDescription();
    assert.match(description, /provider routing policy/i);
    assert.match(description, /Anthropic, OpenAI, or local inference/);
  });

  it("describes /sonnet as a balanced capability tier", () => {
    const description = buildTierCommandDescription("sonnet");
    assert.match(description, /Magos \[sonnet\]/);
    assert.match(description, /balanced capability tier/i);
    assert.match(description, /provider-aware routing/i);
  });

  it("describes /opus as a deep reasoning tier", () => {
    const description = buildTierCommandDescription("opus");
    assert.match(description, /Archmagos \[opus\]/);
    assert.match(description, /deep reasoning tier/i);
  });

  it("can clamp thinking to a selected candidate ceiling", () => {
    assert.equal(clampThinkingLevel("high", "medium"), "medium");
  });
});

describe("classifyRecoveryFailure", () => {
  it("classifies upstream server_error as retryable transient flakiness", () => {
    const classified = classifyRecoveryFailure("assistant error: upstream server_error from provider");
    assert.equal(classified.classification, "transient_server_error");
    assert.equal(classified.retryable, true);
  });

  it("classifies rate limits as failover guidance, not same-model retry", () => {
    const classified = classifyRecoveryFailure("429 rate limit, try again later");
    assert.equal(classified.classification, "rate_limited");
    assert.equal(classified.retryable, false);
    assert.match(classified.guidance, /alternate candidate|blind retry/i);
  });

  it("preserves classification-specific guidance for non-retryable failures", () => {
    const auth = classifyRecoveryFailure("invalid api key");
    const malformed = classifyRecoveryFailure("schema validation failed: malformed json");
    const overflow = classifyRecoveryFailure("maximum context length exceeded");

    assert.equal(auth.classification, "authentication_failed");
    assert.match(auth.guidance, /refresh credentials/i);
    assert.equal(malformed.classification, "malformed_output");
    assert.match(malformed.guidance, /prompt\/schema/i);
    assert.equal(overflow.classification, "context_overflow");
    assert.match(overflow.guidance, /compact context/i);
  });
});

describe("buildRecoveryEvent", () => {
  it("records a structured recovery event for same-model retry-once handling", () => {
    const event = buildRecoveryEvent({
      provider: "openai",
      model: "gpt-5.4",
      turnIndex: 7,
      errorMessage: "server_error: upstream 503 overloaded",
      retryCount: 0,
      guidance: "retry once on the same model",
    });

    assert.equal(event.provider, "openai");
    assert.equal(event.model, "gpt-5.4");
    assert.equal(event.turnIndex, 7);
    assert.equal(event.classification, "transient_server_error");
    assert.equal(event.disposition, "retry_same_model");
    assert.equal(event.retryAttempted, true);
    assert.equal(event.retryCount, 1);
    assert.equal(event.maxRetries, 1);
    assert.match(event.originalErrorSummary, /server_error/i);
  });

  it("does not loop indefinitely after the single retry budget is consumed", () => {
    const event = buildRecoveryEvent({
      provider: "openai",
      model: "gpt-5.4",
      turnIndex: 7,
      errorMessage: "server_error: upstream 503 overloaded",
      retryCount: 1,
      guidance: "single retry already used",
    });

    assert.equal(event.disposition, "escalate");
    assert.equal(event.retryAttempted, false);
    assert.equal(event.retryCount, 1);
  });

  it("captures alternate candidate guidance for failover-visible recovery state", () => {
    const event = buildRecoveryEvent({
      provider: "anthropic",
      model: "claude-sonnet-4-6",
      turnIndex: 4,
      errorMessage: "429 rate limit, try again later",
      retryCount: 0,
      guidance: "Fail over to openai/gpt-5.3-codex-spark for magos.",
      alternateCandidate: { provider: "openai", id: "gpt-5.3-codex-spark" },
      cooldownApplied: true,
    });

    assert.equal(event.classification, "rate_limited");
    assert.equal(event.disposition, "cooldown_and_failover");
    assert.equal(event.cooldownApplied, true);
    assert.deepEqual(event.alternateCandidate, {
      provider: "openai",
      model: "gpt-5.3-codex-spark",
    });
  });
});
