// The docs pages as plain Markdown, built from the same .md files as the HTML pages:
// /docs/<page>.md for each page and /llms-full.txt for all of them.
import { type DocPage, readyDocs, sourceUrl } from "../data/docs";
import { SITE_URL } from "./site";

const sources = import.meta.glob("../pages/docs/*.md", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export const docMdPath = (slug: string): string =>
  slug === "" ? "/docs/index.md" : `/docs/${slug}.md`;

function body(slug: string): string {
  const file = `../pages/docs/${slug === "" ? "index" : slug}.md`;
  const raw = sources[file];
  if (raw === undefined) throw new Error(`no docs page ${file}`);
  return raw.replace(/^---\n[\s\S]*?\n---\n/, "").trim();
}

// Site-relative links such as `/docs/risk/#x` become absolute links to the Markdown page.
function absoluteLinks(md: string): string {
  return md.replace(
    /\]\(\/docs\/([a-z-]*)\/?(#[^)]*)?\)/g,
    (_m, slug: string, frag?: string) => `](${SITE_URL}${docMdPath(slug)}${frag ?? ""})`,
  );
}

export function docMarkdown(page: DocPage): string {
  return [
    `# ${page.title}`,
    "",
    `> ${page.summary}`,
    "",
    absoluteLinks(body(page.slug)),
    "",
    "## Sources",
    "",
    ...page.sources.map((s) => `- ${sourceUrl(s)}`),
    "",
  ].join("\n");
}

export function buildLlmsFull(): string {
  const header = [
    "# keelstack docs",
    "",
    "> Every page of https://keelstack.dev/docs/ as Markdown, in reading order. Index: https://keelstack.dev/llms.txt",
    "",
  ].join("\n");
  return [header, ...readyDocs().map(docMarkdown)].join("\n");
}
