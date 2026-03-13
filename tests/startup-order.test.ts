import { describe, it } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

interface PiPackageJson {
  pi?: {
    extensions?: string[];
  };
}

describe("startup extension order", () => {
  it("registers offline-driver before effort so local driver models exist at effort startup", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const pkgPath = join(here, "..", "package.json");
    const pkg = JSON.parse(readFileSync(pkgPath, "utf-8")) as PiPackageJson;
    const extensions = pkg.pi?.extensions ?? [];
    const offlineIndex = extensions.indexOf("./extensions/offline-driver.ts");
    const effortIndex = extensions.indexOf("./extensions/effort");

    assert.notEqual(offlineIndex, -1, "offline-driver extension must be registered");
    assert.notEqual(effortIndex, -1, "effort extension must be registered");
    assert.ok(offlineIndex < effortIndex, "offline-driver must load before effort");
  });
});
