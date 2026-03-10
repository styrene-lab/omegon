import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { writeJsonlIfChanged } from "./jsonl-io.ts";

describe("writeJsonlIfChanged", () => {
  it("does not rewrite facts.jsonl when content is unchanged", () => {
    let writes = 0;
    const fsSync = {
      existsSync: (_path: string) => true,
      readFileSync: (_path: string, _encoding: string) => "same-content\n",
      writeFileSync: (_path: string, _content: string, _encoding: string) => {
        writes += 1;
      },
    };

    const changed = writeJsonlIfChanged(fsSync as any, "/tmp/facts.jsonl", "same-content\n");
    assert.equal(changed, false);
    assert.equal(writes, 0);
  });

  it("rewrites facts.jsonl when content differs", () => {
    let writes = 0;
    let written = "";
    const fsSync = {
      existsSync: (_path: string) => true,
      readFileSync: (_path: string, _encoding: string) => "old-content\n",
      writeFileSync: (_path: string, content: string, _encoding: string) => {
        writes += 1;
        written = content;
      },
    };

    const changed = writeJsonlIfChanged(fsSync as any, "/tmp/facts.jsonl", "new-content\n");
    assert.equal(changed, true);
    assert.equal(writes, 1);
    assert.equal(written, "new-content\n");
  });
});
