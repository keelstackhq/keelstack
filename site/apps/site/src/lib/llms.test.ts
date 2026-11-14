import { describe, expect, it } from "vite-plus/test";
import { readyDocs } from "../data/docs";
import { docMdPath } from "./docs-md";
import { tools } from "../data/tools";
import { buildLlmsTxt } from "./llms";

describe("buildLlmsTxt", () => {
  const out = buildLlmsTxt();

  it("starts with the title and summary", () => {
    expect(out.startsWith("# keelstack\n\n> ")).toBe(true);
  });

  it("lists each tool with status and repo", () => {
    expect(out).toContain("## Tools");
    for (const t of tools) {
      expect(out).toContain(t.name);
      expect(out).toContain(t.status);
      expect(out).toContain(t.repo);
    }
  });

  it("lists the ready docs pages between start here and tools", () => {
    expect(out).toContain("## Docs");
    for (const d of readyDocs()) {
      expect(out).toContain(
        `- [${d.title}](https://keelstack.dev${docMdPath(d.slug)}): ${d.summary}`,
      );
    }
    expect(out.indexOf("## Start here")).toBeLessThan(out.indexOf("## Docs"));
    expect(out.indexOf("## Docs")).toBeLessThan(out.indexOf("## Tools"));
  });

  it("points agents at llms-full.txt", () => {
    expect(out).toContain("https://keelstack.dev/llms-full.txt");
  });

  it("names the docspack package on npm for offline use", () => {
    expect(out).toContain("https://www.npmjs.com/package/@keelstackhq/docspack");
    expect(out).not.toContain("keelstack.dev/docspack/");
  });

  it("lists all six tools with their versions", () => {
    for (const t of tools) {
      expect(out).toContain(`- [${t.name}](${t.repo}) (${t.status}): `);
    }
  });

  it("stays an index: under 6 KB and without a copied command block", () => {
    expect(out.length).toBeLessThan(6000);
    expect(out).not.toContain("```");
  });
});

describe("buildLlmsTxt start here", () => {
  const out = buildLlmsTxt();
  it("points at /docs/start.md and states the agent contract", () => {
    expect(out).toContain("## Start here");
    expect(out).toContain("https://keelstack.dev/docs/start.md");
    expect(out).toContain("`ok`, `exit_code`, `data`, `error.fix`, `next`");
    expect(out).toContain("6 limit");
  });
});

describe("buildLlmsTxt after bosun 0.11.0", () => {
  it("says nothing about bosun main after 0.10.0", () => {
    expect(buildLlmsTxt()).not.toContain("after 0.10.0");
  });
});
