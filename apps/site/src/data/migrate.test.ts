import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { footerLinks, nav } from "./content";
import { migrate, RUNBOOK_URL } from "./migrate";
import { buildLlmsTxt } from "../lib/llms";

const page = readFileSync(new URL("../pages/migrate.astro", import.meta.url), "utf8");

test("the knob table has the nine booster knobs with the runbook's values", () => {
  expect(migrate.knobs.map((k) => k[0])).toEqual([
    "[approvals] policy",
    "push_gates",
    "heal_pr",
    "heal_main",
    "approvals_batch",
    "spawn_auto",
    "budget_feature",
    "brake",
    "curator_loosen",
  ]);
  const by = Object.fromEntries(migrate.knobs.map((k) => [k[0], [k[1], k[2]]]));
  expect(by.heal_pr).toEqual(["notify", "spawn"]);
  expect(by.spawn_auto).toEqual(["off", "on"]);
  expect(by.budget_feature).toEqual(["warn", "ask"]);
  expect(by.curator_loosen).toEqual(["off", "propose"]);
});

test("three environments each end with a prompt that reads the runbook and never merges", () => {
  for (const env of [migrate.local, migrate.web, migrate.slack]) {
    expect(env.prompt).toContain("docs/migrate-booster.md");
    expect(env.prompt).toMatch(/[Nn]ever merge, tag or force-push/);
  }
  expect(migrate.web.prompt).toContain("no PAT");
  expect(migrate.slack.prompt).toContain("never in Slack");
});

test("web section covers setup script, hosts, env vs secrets and [cloud] environment_id", () => {
  expect(migrate.web.setup).toBe("bosun setup-snippet");
  expect(migrate.web.hosts.map((h) => h[0])).toEqual([
    "registry.npmjs.org",
    "deb.nodesource.com",
    "github.com",
    "tuf-repo-cdn.sigstore.dev",
  ]);
  expect(migrate.web.cloudToml).toContain("[cloud]");
  expect(migrate.web.cloudToml).toContain("environment_id");
  expect(migrate.web.env).toContain("No GitHub PAT");
});

test("page: three headings, runbook link, no new island", () => {
  expect(page).toContain("href={RUNBOOK_URL}");
  expect(RUNBOOK_URL).toBe(
    "https://github.com/keelstackhq/bosun/blob/main/docs/migrate-booster.md",
  );
  expect(page).not.toMatch(/client:/);
  for (const k of ["local", "web", "slack"]) expect(page).toContain(`{${k}.heading}`);
});

test("linked from the nav, the footer, the home Get started links and llms.txt", () => {
  expect(nav.some((l) => l.href === "/migrate/")).toBe(true);
  expect(footerLinks.some((l) => l.href === "/migrate/")).toBe(true);
  const out = buildLlmsTxt();
  expect(out).toContain("## Move a repo to bosun booster");
  expect(out).toContain("/migrate/");
  expect(out).toContain(RUNBOOK_URL);
});

test("copy never names a client org or repo", () => {
  const src = readFileSync(new URL("./migrate.ts", import.meta.url), "utf8") + page;
  const orgs = [...src.matchAll(/github\.com\/([A-Za-z0-9_.-]+)/g)].map((m) => m[1]);
  expect(orgs.length).toBeGreaterThan(0);
  expect(new Set(orgs)).toEqual(new Set(["keelstackhq"]));
});
