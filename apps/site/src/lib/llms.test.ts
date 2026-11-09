import { describe, expect, it } from "vite-plus/test";
import { readyDocs } from "../data/docs";
import { docMdPath } from "./docs-md";
import { agentRows } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTION, DEFAULT_ON } from "./command";
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

  it("lists the agent keys", () => {
    expect(out).toContain("## For agents");
    expect(agentRows).toHaveLength(7);
    for (const r of agentRows) expect(out).toContain(r.key);
  });

  it("lists the ready docs pages between agents and get started", () => {
    expect(out).toContain("## Docs");
    for (const d of readyDocs()) {
      expect(out).toContain(
        `- [${d.title}](https://keelstack.dev${docMdPath(d.slug)}): ${d.summary}`,
      );
    }
    expect(out.indexOf("## For agents")).toBeLessThan(out.indexOf("## Docs"));
    expect(out.indexOf("## Docs")).toBeLessThan(out.indexOf("## Get started"));
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

  it("includes the splice command and its caption", () => {
    expect(out).toContain("## Get started");
    expect(out).toContain(buildCommand(new Set(DEFAULT_ON)));
    expect(out).toContain(`\n${CAPTION}\n`);
    expect(out).not.toContain("render.mjs");
  });
});

describe("buildLlmsTxt start in 5 minutes", () => {
  const out = buildLlmsTxt();
  it("lists the human steps and the agent contract before Tools", () => {
    expect(out).toContain("## Start in 5 minutes");
    expect(out.indexOf("## Start in 5 minutes")).toBeLessThan(out.indexOf("## Tools"));
    expect(out).toContain("1. `npm i -g @keelstackhq/bosun`");
    expect(out).toContain("9. ");
    expect(out).toContain("`ok`, `exit_code`, `data`, `error.fix`, `next`");
    expect(out).toContain("6 limit");
    expect(out).toContain("https://keelstack.dev/docs/start.md");
  });
});

describe("buildLlmsTxt after bosun 0.11.0", () => {
  it("says nothing about bosun main after 0.10.0", () => {
    expect(buildLlmsTxt()).not.toContain("after 0.10.0");
  });
});
