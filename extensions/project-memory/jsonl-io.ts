export function writeJsonlIfChanged(
  fsSync: Pick<typeof import("node:fs"), "existsSync" | "readFileSync" | "writeFileSync">,
  jsonlPath: string,
  nextJsonl: string,
): boolean {
  const existing = fsSync.existsSync(jsonlPath)
    ? fsSync.readFileSync(jsonlPath, "utf8")
    : null;
  if (existing === nextJsonl) return false;
  fsSync.writeFileSync(jsonlPath, nextJsonl, "utf8");
  return true;
}
