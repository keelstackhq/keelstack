import { facts } from "./facts";
import { bare, sources, versions } from "./versions";
import { expect, test } from "vite-plus/test";
import * as content from "./content";
import { tools } from "./tools";

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

test("terminal shows the default command and rows generated from hull's pieces", () => {
  expect(terminal.title).toBe(`~ · splice ${bare(versions.splice)}`);
  expect(terminal.command).toEqual([
    `pnpm dlx @keelstackhq/splice@${bare(versions.splice)} new \\`,
    `${sources.hull}/template my-repo --ref ${versions.hull} \\`,
    "--piece preact-app:web --piece hono-worker:api",
  ]);
  expect(terminal.output.map((o) => o.path)).toEqual([
    ".  (base)",
    "web/  (preact-app)",
    "api/  (hono-worker)",
  ]);
  expect(terminal.summary).toBe(
    `# … ${(["base", "preact-app", "hono-worker"] as const).reduce((a, id) => a + facts.templateFiles[id]!, 0)} template files from 3 pieces; then pnpm install`,
  );
});

test("stats shapes", () => {
  expect(stats).toHaveLength(4);
  for (const s of stats) expect(s.value.startsWith("1 ")).toBe(true);
});

test("split has 12 chips", () => {
  expect(split.handles).toHaveLength(12);
});

test("agent rows and current cards", () => {
  expect(agentRows.map((r) => r.key)).toEqual([
    "CLAUDE.md",
    "ready",
    "bosun",
    "hoist",
    "--json --yes",
    "ci-ok",
    "update bot",
    "Manifest",
  ]);
  expect(current.map((c) => c.title)).toEqual(["Replaced", "3-way merged", "Stay yours"]);
  expect(current.map((c) => !!c.accent)).toEqual([false, false, true]);
});

test("hoist row: the org control plane is available, not coming", () => {
  const row = agentRows.find((r) => r.key === "hoist");
  expect(row?.text).toContain("`hoist org plan|check|apply`");
  expect(row?.text).not.toMatch(/coming/);
});

test("hoist row: the org subcommands come from hoist's OrgCommand enum (D-024)", () => {
  const row = agentRows.find((r) => r.key === "hoist");
  expect(facts.hoistOrg).toEqual(["plan", "check", "apply"]);
  expect(row?.text).toContain(`\`hoist org ${facts.hoistOrg.join("|")}\``);
});

test("CLAUDE.md row: hull ships the subagents, no rigging step since hull v0.11.0", () => {
  const row = agentRows.find((r) => r.key === "CLAUDE.md");
  expect(row?.text).toContain("hull ships the subagents");
  expect(row?.text).not.toContain("rigging");
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
  expect(footerLinks.map((l) => l.label)).toEqual([
    ...tools.map((t) => t.name),
    "decisions",
    "Migrate to booster",
  ]);
  expect(footerLinks.find((l) => l.label === "decisions")?.href).toBe(
    `${facts.repoUrl}/tree/main/decisions`,
  );
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

test("get started links Start a project and the greenfield bosun page", () => {
  expect(sections.start.links?.map((l) => l.href)).toEqual([
    "/docs/start-project/",
    "/docs/greenfield-bosun/",
    "/docs/adopt/",
    "/migrate/",
  ]);
});

test("nav links the manifest before Docs", () => {
  const labels = nav.map((l) => l.label);
  expect(labels.indexOf("Manifest")).toBe(labels.indexOf("Docs") - 1);
  expect(nav.find((l) => l.label === "Manifest")?.href).toBe("/docs/operating-model/");
  expect(agentRows.find((r) => r.key === "Manifest")?.href).toBe("/docs/operating-model/");
});

test("start in 5 minutes: the nine human steps in order, and the agent contract", () => {
  const { quickstart } = content;
  expect(quickstart.steps).toHaveLength(9);
  expect(quickstart.steps.map((s) => s.cmd ?? null)).toEqual([
    "npm i -g @keelstackhq/bosun",
    "export GITHUB_TOKEN=$(gh auth token)",
    "bosun adopt --owner <you>",
    "bosun keygen --enroll",
    null,
    null,
    "bosun setup-snippet",
    "bosun doctor",
    null,
  ]);
  expect(quickstart.more.href).toBe("/docs/start/");
  expect(quickstart.agents.fields).toEqual(["ok", "exit_code", "data", "error.fix", "next"]);
  expect(quickstart.agents.exitCodes).toEqual([
    [0, "ok"],
    [1, "error"],
    [2, "usage"],
    [3, "conflict"],
    [4, "halted"],
    [5, "needs-human"],
    [6, "limit"],
  ]);
});

test("nav links the 5-minute start first", () => {
  expect(nav[0]).toEqual({ label: "Start", href: "#quickstart" });
});

test("start in 5 minutes runs on released bosun: no unreleased note", () => {
  expect("unreleased" in content.quickstart).toBe(false);
});
