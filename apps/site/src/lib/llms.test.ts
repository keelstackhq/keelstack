import { describe, expect, it } from "vite-plus/test";
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
    expect(agentRows).toHaveLength(4);
    for (const r of agentRows) expect(out).toContain(r.key);
  });

  it("includes the splice command and its caption", () => {
    expect(out).toContain("## Get started");
    expect(out).toContain(buildCommand(new Set(DEFAULT_ON)));
    expect(out).toContain(`\n${CAPTION}\n`);
    expect(out).not.toContain("render.mjs");
  });
});
