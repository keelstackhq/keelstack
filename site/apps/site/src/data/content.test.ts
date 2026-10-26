import { expect, test } from "vite-plus/test";
import * as content from "./content";
import { org, tools } from "./tools";

const { agentRows, current, footerLinks, hero, nav, sections, split, stats, terminal } = content;

function strings(v: unknown): string[] {
  if (typeof v === "string") return [v];
  if (Array.isArray(v)) return v.flatMap(strings);
  if (v && typeof v === "object") return Object.values(v).flatMap(strings);
  return [];
}

test("section labels run 01..05 in order", () => {
  const keys = ["split", "tools", "agents", "current", "start"] as const;
  keys.forEach((k, i) => {
    expect(sections[k].label).toMatch(new RegExp(`^0${i + 1} · `));
  });
});

test("hero has three variants and seconds is active", () => {
  expect(Object.keys(hero.variants)).toEqual(["seconds", "skip", "agent"]);
  expect(hero.active).toBe("seconds");
});

test("terminal shows the default command, real create lines and file counts", () => {
  expect(terminal.title).toContain("splice 0.2.0");
  expect(terminal.title).not.toMatch(/preview/i);
  // The command that was run to record the output below.
  expect(terminal.command).toEqual([
    "pnpm dlx @keelstackhq/splice@0.2.0 new \\",
    "gh:keelstackhq/hull//template my-repo --ref v0.2.0 \\",
    "--piece preact-app:web --piece hono-worker:api",
  ]);
  // Lines copied from `splice new` output (hull v0.2.0, splice 0.2.0), in output order.
  expect(terminal.output.map((o) => `create  ${o.path}  ${o.cls}`)).toEqual([
    "create  .github/workflows/ci.yml  owned",
    "create  AGENTS.md  seed",
    "create  CLAUDE.md  owned",
    "create  apps/api/src/index.ts  seed",
    "create  apps/web/src/app.tsx  seed",
  ]);
  expect(terminal.summary).toBe("# … 53 files: 31 owned, 8 shared, 14 seed; then pnpm install");
});

test("stats shapes", () => {
  expect(stats).toHaveLength(4);
  for (const s of stats) expect(s.value.startsWith("1 ")).toBe(true);
});

test("split has 12 chips", () => {
  expect(split.handles).toHaveLength(12);
});

test("agent rows and current cards", () => {
  expect(agentRows.map((r) => r.key)).toEqual(["CLAUDE.md", "--json --yes", "ci-ok", "update bot"]);
  expect(current.map((c) => c.title)).toEqual(["Replaced", "3-way merged", "Stay yours"]);
  expect(current.map((c) => !!c.accent)).toEqual([false, false, true]);
});

test("CLAUDE.md row: subagents come from the rigging channel since hull v0.2.0", () => {
  const row = agentRows.find((r) => r.key === "CLAUDE.md");
  expect(row?.text).toContain("rigging channel");
  expect(row?.text).not.toMatch(/Subagents, skills/);
});

test("links stay in the keelstackhq org or on the page", () => {
  for (const l of [...footerLinks, ...nav]) {
    expect(
      l.href.startsWith("#") ||
        l.href.startsWith("/") ||
        l.href.startsWith("https://github.com/keelstackhq"),
    ).toBe(true);
  }
  expect(footerLinks.map((l) => l.label)).toEqual(["hull", "splice", "hoard", "decisions"]);
  expect(footerLinks[3]?.href).toBe(`${org.url}/decisions`);
});

test("get started copy no longer says splice is unreleased", () => {
  expect(sections.start.body).not.toMatch(/not released|dev renderer/);
  expect(sections.start.body).toContain("pnpm install");
});

test("no old org names and no empty strings", () => {
  const all = [...strings(content), ...strings(tools)];
  const text = JSON.stringify(all);
  expect(text).not.toMatch(/@keelstack\//);
  expect(text).not.toMatch(/github\.com\/keelstack\//);
  expect(text).not.toMatch(/gh:keelstack\//);
  expect(all.every((s) => s.length > 0)).toBe(true);
});
