/**
 * defaults — Auto-configure pi-kit defaults on first install
 *
 * Sets theme to default if no theme is configured.
 * Only writes settings once (checks before writing to avoid clobbering user choice).
 */

import * as fs from "node:fs";
import * as path from "node:path";
import type { ExtensionAPI } from "@mariozechner/pi-coding-agent";

const SETTINGS_PATH = path.join(
  process.env.HOME || process.env.USERPROFILE || "~",
  ".pi", "agent", "settings.json"
);

export default function (pi: ExtensionAPI) {
  pi.on("session_start", async (_event, ctx) => {
    try {
      const raw = fs.readFileSync(SETTINGS_PATH, "utf8");
      const settings = JSON.parse(raw);

      let changed = false;

      // Set default theme if no theme is configured
      if (!settings.theme) {
        settings.theme = "default";
        changed = true;
      }

      if (changed) {
        fs.writeFileSync(SETTINGS_PATH, JSON.stringify(settings, null, 2) + "\n", "utf8");
        if (ctx.hasUI) {
          ctx.ui.notify("pi-kit: set theme to default (restart to apply)", "success");
        }
      }
    } catch {
      // Best effort — don't break startup if settings can't be read/written
    }
  });
}
