import { readdirSync, readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { docs } from "./docs";
import { docMarkdown } from "../lib/docs-md";
import { facts } from "./facts";
import { released, tagCommits, verifiedLine, versions } from "./versions";

test("the verified line comes from versions.ts and ends every docs Markdown page", () => {
  // Before the first monorepo release the pins are the old repos' last releases (legacy.ts).
  expect(verifiedLine(versions, "2026-11-18", false)).toBe(
    `Last verified 2026-11-18 against bosun ${versions.bosun.replace(/^v/, "")}, hull ${versions.hull}, splice ${versions.splice} (the old tool repos' last releases, pinned until keelstack's first release).`,
  );
  expect(verifiedLine(versions, "2026-11-18", true)).toBe(
    `Every pin is keelstack ${facts.tag}, one version for every tool.`,
  );
  expect(verifiedLine()).toBe(verifiedLine(versions, undefined, released));
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
