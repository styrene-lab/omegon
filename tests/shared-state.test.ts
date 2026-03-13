import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { sharedState } from "../extensions/shared-state.ts";

describe("sharedState lifecycle candidate queue", () => {
  it("supports lightweight lifecycle candidate queue entries", () => {
    const original = sharedState.lifecycleCandidateQueue;
    sharedState.lifecycleCandidateQueue = [];

    sharedState.lifecycleCandidateQueue.push({
      source: "design-tree",
      context: "Decided design decision",
      candidates: [{
        sourceKind: "design-decision",
        authority: "explicit",
        section: "Decisions",
        content: "Use hybrid lifecycle-driven memory writes",
        artifactRef: {
          type: "design-node",
          path: "docs/memory-lifecycle-integration.md",
          subRef: "Use hybrid lifecycle-driven memory writes",
        },
      }],
    });

    assert.equal(sharedState.lifecycleCandidateQueue.length, 1);
    assert.equal(sharedState.lifecycleCandidateQueue[0].source, "design-tree");
    assert.equal(sharedState.lifecycleCandidateQueue[0].candidates[0].section, "Decisions");

    sharedState.lifecycleCandidateQueue = original;
  });
});
