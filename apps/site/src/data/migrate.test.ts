import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { footerLinks, nav } from "./content";
import { APPENDIX_URL, migrate, RUNBOOK_URL } from "./migrate";
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

test("leads with the setup script from the guide, as is, then the 2-line prompt, then one PR", () => {
  expect(migrate.quick.setup).toBe(
    [
      "for d in . ./*/; do",
      '  [ -f "$d/.bosun/version" ] && [ -f "$d/docs/setup-script.sh" ] || continue',
      '  (cd "$d" && timeout -k 10 1800 bash docs/setup-script.sh </dev/null) || echo "bosun: $d/docs/setup-script.sh failed" >&2',
      "done; exit 0",
    ].join("\n"),
  );
  expect(migrate.quick.prompt).toBe(
    [
      "Run `bosun migrate --to booster --direct`; do what its `next` and `data.then` say; stop when it asks for the owner.",
      "After the merge run `bosun migrate --finish`.",
    ].join("\n"),
  );
  expect(migrate.quick.prompt.split("\n")).toHaveLength(2);
  expect(migrate.quick.approve).toContain("one PR");
  const order = ["{quick.setup}", "{quick.prompt}", "{quick.approve}", '<h2 id="local">'].map((k) =>
    page.indexOf(k),
  );
  expect(order.every((n) => n > 0)).toBe(true);
  expect(order).toEqual(order.toSorted((a, b) => a - b));
});

test("network note: Limited with package managers, github.com and release-assets for Rust", () => {
  expect(migrate.quick.network).toContain("Limited");
  expect(migrate.quick.network).toContain("Allow package managers");
  expect(migrate.quick.network).toContain("github.com");
  expect(migrate.quick.network).toContain("release-assets.githubusercontent.com");
  expect(migrate.quick.network).toContain("Rust");
});

test("requires bosun 0.15.0 and hull v0.10.0, and says so", () => {
  expect(migrate.forWho).toContain("bosun 0.15.0");
  expect(migrate.forWho).toContain("hull v0.10.0");
  expect(JSON.stringify(migrate)).not.toContain("0.14.0");
});

test("CLI, web and Slack sections are short, one prompt for all, long path links to the appendix", () => {
  for (const env of [migrate.local, migrate.web, migrate.slack]) {
    expect(env.steps.length).toBeLessThanOrEqual(5);
  }
  expect(migrate.slack.heading).toContain("Slack");
  expect(APPENDIX_URL).toBe(`${RUNBOOK_URL}#appendix-a-the-manual-path`);
  expect(page).toContain("href={APPENDIX_URL}");
  expect(migrate.web.steps.join(" ")).toContain("No secrets, no PAT");
});

test("page: env headings, runbook link, no new island", () => {
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
