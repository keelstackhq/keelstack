import { describe, expect, it } from "vite-plus/test";
import docspack from "../../docspack.json";
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
    expect(agentRows).toHaveLength(5);
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

  it("names the docspack tarball for offline use", () => {
    expect(out).toContain(
      `https://keelstack.dev/docspack/keelstackhq-docspack-${docspack.version}.tgz`,
    );
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
