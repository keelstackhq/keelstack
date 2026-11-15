import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { footerLinks, nav } from "./content";
import { APPENDIX_URL, migrate, RUNBOOK_URL } from "./migrate";
import { bare, versions } from "./versions";
import { buildLlmsTxt } from "../lib/llms";

const page = readFileSync(new URL("../pages/migrate.astro", import.meta.url), "utf8");

test("the ladder has the four modes in order, flags, rollback line, and links the knob table", () => {
  expect(migrate.ladder.map((r) => r[0])).toEqual([
    "gatekeeper",
    "steady",
    "booster",
    "autonomous",
  ]);
  expect(JSON.stringify(migrate)).not.toContain("bosun pr --pr <n> --ready");
  expect(migrate.does.startId).toBe("c");
  expect(migrate.notes.join(" ")).toContain("Rollback");
  expect(migrate.knobsUrl).toContain("bosun/blob/main/docs/quickstart.md");
  expect(page).toContain("{migrate.ladder.map");
  expect(page).not.toContain("migrate.knobs.map");
});

test("leads with a pointer to the one setup script, then the 2-line prompt, then one PR", () => {
  expect(migrate.quick.setupId).toBe("e");
  expect(JSON.stringify(migrate)).not.toContain("for d in . ./*/");
  expect(migrate.quick.prompt).toBe(
    [
      "Run `bosun migrate --to booster --direct`; do what its `next` and `data.then` say; stop when it asks for the owner.",
      "After the merge run `bosun migrate --finish`.",
    ].join("\n"),
  );
  expect(migrate.quick.prompt.split("\n")).toHaveLength(2);
  expect(migrate.quick.approve).toContain("one PR");
  const order = ["{quick.setupLabel}", "{quick.prompt}", "{quick.approve}", '<h2 id="local">'].map(
    (k) => page.indexOf(k),
  );
  expect(order.every((n) => n > 0)).toBe(true);
  expect(order).toEqual(order.toSorted((a, b) => a - b));
});

test("requires bosun 0.15.0 and says it pairs with hull v0.10.0", () => {
  expect(migrate.forWho).toContain(`bosun ${bare(versions.bosun)}`);
  expect(migrate.forWho).toContain("pairs with hull v0.10.0");
  expect(migrate.forWho).not.toMatch(/hull v0\.10\.0 or newer/);
  expect(JSON.stringify(migrate)).not.toContain("0.14.0");
});

test("CLI, web and Slack sections are short, one prompt for all, long path links to the appendix", () => {
  for (const env of [migrate.local, migrate.web, migrate.slack]) {
    expect(env.steps.length).toBeLessThanOrEqual(5);
  }
  expect(migrate.slack.heading).toContain("Slack");
  expect(APPENDIX_URL).toBe(`${RUNBOOK_URL}#appendix-a-the-manual-path`);
  expect(page).toContain("href={APPENDIX_URL}");
  expect(migrate.web.steps.join(" ")).toContain("Start block E");
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
  expect(out).toContain("/migrate.md");
});

test("copy never names a client org or repo", () => {
  const src = readFileSync(new URL("./migrate.ts", import.meta.url), "utf8") + page;
  const orgs = [...src.matchAll(/github\.com\/([A-Za-z0-9_.-]+)/g)].map((m) => m[1]);
  expect(orgs.length).toBeGreaterThan(0);
  expect(new Set(orgs)).toEqual(new Set(["keelstackhq"]));
});

test("migrate.md: every keelstack.dev URL has one origin and ends in .md or is the page itself", async () => {
  const { GET } = await import("../pages/migrate.md");
  const md = await GET().text();
  expect(md).not.toMatch(/https:\/\/[^\s]*https:\/\//);
  const own = [...md.matchAll(/https:\/\/keelstack\.dev\/[^\s)]*/g)].map((m) => m[0]);
  expect(own).toContain("https://keelstack.dev/docs/start/e.md");
  expect(own).toContain("https://keelstack.dev/docs/start/c.md");
  for (const u of own) expect(u).toMatch(/\.md$/);
});
