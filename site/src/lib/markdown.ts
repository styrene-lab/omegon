export function stripFrontmatter(markdown: string): string {
  const text = markdown.replace(/^\uFEFF/, "");
  const match = text.match(/^(---|\+\+\+)\r?\n[\s\S]*?\r?\n\1\r?\n?/);
  return match ? text.slice(match[0].length) : text;
}
