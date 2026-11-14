import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { docs } from "./docs";
import { docMarkdown } from "../lib/docs-md";
import { verifiedLine } from "./versions";

test("the verified line comes from versions.ts and ends every docs Markdown page", () => {
  expect(verifiedLine()).toBe("Last verified against bosun 0.15.0, hull v0.10.0, splice 0.4.0.");
  for (const d of docs.filter((x) => x.ready)) {
    expect(docMarkdown(d).trimEnd().endsWith(verifiedLine())).toBe(true);
  }
});

test("no page states its own 'as of' or 'last verified' line: one footer component does", () => {
  const dir = new URL("../pages/docs/", import.meta.url);
  for (const f of readdirSync(dir).filter((n) => n.endsWith(".md"))) {
    const t = readFileSync(new URL(f, dir), "utf8");
    expect(t, f).not.toMatch(/last verified|releases as of|released ones as of/i);
  }
  const base = readFileSync(new URL("../layouts/Base.astro", import.meta.url), "utf8");
  expect(base).toContain("<Verified />");
});
