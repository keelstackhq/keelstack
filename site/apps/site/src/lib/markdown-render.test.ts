import { readFileSync, readdirSync } from "node:fs";
import { createSatteriMarkdownProcessor } from "@astrojs/markdown-satteri";
import { expect, test } from "vite-plus/test";
import { markdownOptions } from "./markdown";

const dir = new URL("../pages/docs/", import.meta.url);
const pages = readdirSync(dir).filter((f) => f.endsWith(".md"));

// Render with the same processor options the site builds with.
const render = async (md: string): Promise<string> => {
  const processor = await createSatteriMarkdownProcessor(markdownOptions);
  return (await processor.render(md)).code;
};

test("docs prose keeps `--flag` as two hyphens, not an en dash", async () => {
  const html = await render("Run splice update --all, or --channel bosun.");
  expect(html).toContain("--all");
  expect(html).toContain("--channel");
  expect(html).not.toContain("–");
});

test("docs pages keep smart quotes", async () => {
  expect(await render("bosun's own repo")).toContain("bosun’s");
});

test("no glob such as `.bosun/**` renders as bold in a docs page", async () => {
  for (const f of pages) {
    const html = await render(readFileSync(new URL(f, dir), "utf8"));
    // `/<strong>` only happens when the `**` of a glob opens emphasis.
    expect(html, f).not.toContain("/<strong>");
  }
});
