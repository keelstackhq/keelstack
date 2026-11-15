import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { docs } from "./docs";
import { docMarkdown } from "../lib/docs-md";
import { tagCommits, verifiedLine, versions } from "./versions";

test("the verified line comes from versions.ts and ends every docs Markdown page", () => {
  expect(verifiedLine()).toBe(
    "Last verified 2026-11-15 against bosun 0.15.0, hull v0.12.0, splice 0.5.1.",
  );
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

test("every pinned tag commit is a full SHA", () => {
  for (const k of Object.keys(versions) as (keyof typeof versions)[]) {
    expect(tagCommits[k], k).toMatch(/^[0-9a-f]{40}$/);
  }
});
