// "Start a project" (/docs/start-project/) is hull's docs/start-checklist.md, copied verbatim at a
// pinned hull ref. The copy carries its source and sha256, so a hand edit of the copy, or a page
// that no longer matches the copy, fails here. Re-copy with apps/site/scripts/start-checklist.mjs.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docs } from "./docs";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const copy = read("./hull/start-checklist.md");
const source = JSON.parse(read("./hull/start-checklist.json")) as {
  repo: string;
  ref: string;
  path: string;
  sha256: string;
};
const page = read("../pages/docs/start-project.md");

describe("Start a project page", () => {
  it("pins hull's checklist by commit and checksum", () => {
    expect(source.repo).toBe("keelstackhq/hull");
    expect(source.path).toBe("docs/start-checklist.md");
    expect(source.ref).toMatch(/^[0-9a-f]{40}$/);
    expect(createHash("sha256").update(copy).digest("hex")).toBe(source.sha256);
  });

  it("is the copy without its H1, after a note naming the pinned ref", () => {
    const body = copy.replace(/^# .*\n\n/, "");
    expect(page.endsWith(body)).toBe(true);
    expect(page).toContain(
      `https://github.com/keelstackhq/hull/blob/${source.ref}/docs/start-checklist.md`,
    );
  });

  it("is a ready docs page right after Start here", () => {
    const slugs = docs.map((d) => d.slug);
    expect(slugs.indexOf("start-project")).toBe(slugs.indexOf("start-here") + 1);
    const entry = docs.find((d) => d.slug === "start-project")!;
    expect(entry.title).toBe("Start a project");
    expect(entry.ready).toBe(true);
    expect(entry.sources).toContain("hull/docs/start-checklist.md");
  });
});
