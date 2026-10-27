import { describe, expect, it } from "vite-plus/test";
import { readyDocs, sourceUrl } from "../data/docs";
import { buildLlmsFull, docMarkdown, docMdPath } from "./docs-md";

describe("docs as Markdown", () => {
  it("maps each page to a .md path next to its HTML page", () => {
    expect(docMdPath("")).toBe("/docs/index.md");
    expect(docMdPath("playbook")).toBe("/docs/playbook.md");
  });

  it("renders a page as clean Markdown: title, summary, body, sources", () => {
    for (const d of readyDocs()) {
      const md = docMarkdown(d);
      expect(md.startsWith(`# ${d.title}\n\n> ${d.summary}\n\n`)).toBe(true);
      expect(md).not.toMatch(/^layout:/m);
      expect(md).not.toContain("---\nlayout");
      expect(md).toContain("## Sources");
      for (const s of d.sources) expect(md).toContain(sourceUrl(s));
      // Site-relative links become absolute links to the Markdown pages.
      expect(md).not.toMatch(/\]\(\//);
    }
  });

  it("puts every ready page into llms-full.txt, in order", () => {
    const full = buildLlmsFull();
    let at = 0;
    for (const d of readyDocs()) {
      const i = full.indexOf(docMarkdown(d));
      expect(i).toBeGreaterThanOrEqual(at);
      at = i;
    }
  });
});
