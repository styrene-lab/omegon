import { readFileSync } from "node:fs";
import { join } from "node:path";
import { stripFrontmatter } from "./markdown";

/**
 * Read a repo-root markdown document during the site build.
 *
 * The Astro build runs with cwd = site/, so repo-root files live one level
 * up. Do NOT resolve these paths from import.meta.url: during `astro build`
 * it points at the Vite-compiled chunk, not the source file, which is how
 * the changelog page silently shipped a "Not available during this build"
 * placeholder to production.
 *
 * Any read failure throws and fails the build. These files always exist in
 * a full checkout; a missing file means the build context is wrong, and a
 * placeholder page in production is worse than a red CI run.
 */
export function readRepoDoc(filename: string): string {
  const path = join(process.cwd(), "..", filename);
  let raw: string;
  try {
    raw = readFileSync(path, "utf-8");
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    throw new Error(
      `readRepoDoc: failed to read repo-root document "${filename}" at ${path}: ${message}. ` +
        `The site build must run with cwd = site/ inside a full repo checkout.`,
    );
  }
  const stripped = stripFrontmatter(raw);
  if (stripped.trim().length === 0) {
    throw new Error(
      `readRepoDoc: repo-root document "${filename}" at ${path} is empty after frontmatter stripping.`,
    );
  }
  return stripped;
}
