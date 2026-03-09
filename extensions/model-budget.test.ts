import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { buildSetModelTierDescription, buildTierCommandDescription } from "./model-budget.ts";
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
