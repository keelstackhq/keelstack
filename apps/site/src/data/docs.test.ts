import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docHref, docs, readyDocs, sourceUrl } from "./docs";

const dir = new URL("../pages/docs/", import.meta.url);
const repos = ["hull", "splice", "bosun", "hoist", "hoard", "berth", "decisions", "rust-canary"];
const fileFor = (slug: string) => (slug === "" ? "index.md" : `${slug}.md`);
const files = readdirSync(dir).filter((f) => f.endsWith(".md"));

describe("docs data", () => {
  it("has the seven pages in order with unique slugs", () => {
    expect(docs.map((d) => d.slug)).toEqual([
      "",
      "getting-started",
      "tasks",
      "ci",
      "risk",
      "releases",
      "troubleshooting",
    ]);
    expect(docs.map((d) => d.title)).toEqual([
      "Overview",
      "Getting started",
      "Tasks and sessions",
      "CI and gates",
      "Risk and review",
      "Releases and updates",
      "Troubleshooting",
    ]);
    expect(new Set(docs.map((d) => d.slug)).size).toBe(docs.length);
  });

  it("ready entries and page files match", () => {
    for (const d of readyDocs()) expect(files).toContain(fileFor(d.slug));
    const ready = new Set(readyDocs().map((d) => fileFor(d.slug)));
    for (const f of files) expect(ready.has(f)).toBe(true);
  });

  it("ready entries have a summary and valid sources", () => {
    for (const d of readyDocs()) {
      expect(d.summary.trim()).not.toBe("");
      expect(d.sources.length).toBeGreaterThan(0);
      for (const s of d.sources) {
        const [repo, ...rest] = s.split("/");
        expect(repos).toContain(repo);
        expect(rest.join("/")).not.toBe("");
      }
    }
  });

  it("builds hrefs and source urls", () => {
    expect(docHref("")).toBe("/docs/");
    expect(docHref("ci")).toBe("/docs/ci/");
    expect(sourceUrl("bosun/docs/spec.md")).toBe(
      "https://github.com/keelstackhq/bosun/blob/main/docs/spec.md",
    );
  });

  it("page files link only to ready pages and use no old org names", () => {
    const hrefs = new Set(readyDocs().map((d) => docHref(d.slug)));
    for (const f of files) {
      const text = readFileSync(new URL(f, dir), "utf8");
      for (const m of text.matchAll(/(?:\]\(|href=")(\/docs\/[^)"\s]*)/g)) {
        expect(hrefs.has(m[1]!.split("#")[0]!)).toBe(true);
      }
      for (const old of ["@keelstack/", "github.com/keelstack/", "gh:keelstack/"]) {
        expect(text).not.toContain(old);
      }
    }
  });
});
