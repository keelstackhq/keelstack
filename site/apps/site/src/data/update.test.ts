import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { update } from "./content";
import { buildLlmsTxt } from "../lib/llms";
import { startScenarioMd } from "../lib/start-md";

const page = readFileSync(new URL("../pages/docs/update.md", import.meta.url), "utf8");
const llms = buildLlmsTxt();

test("the hull update path is on the page", () => {
  expect(update.hull.steps.map((s) => s.cmd)).toEqual([
    "git switch -c chore/splice-update",
    "splice check --json",
    "splice diff",
    "splice update --all",
    "git grep -n '<<<<<<<'",
    "splice check",
    "pnpm install && pnpm run ci",
    'git add -A && git commit -m "chore: splice update to hull vX.Y.Z"',
  ]);
  expect(update.hull.notes).toEqual([
    "Normally you do not run this: the weekly splice workflow (`splice update --all`) opens the PR on `splice/update`.",
    "Needs splice 0.3.0 or later. Without a global install write `pnpm dlx @keelstackhq/splice@0.4.0` in place of `splice`.",
    "Do bosun first, then hull: each is its own PR, each is R-gov, and the owner approves each head.",
    "Merge only when `ci-ok` is green.",
  ]);
});

test("the bosun update path is on the page", () => {
  expect(update.bosun.steps.map((s) => s.cmd ?? null)).toEqual([
    "bosun doctor",
    "bosun pin --latest --check",
    "bosun pin --latest --pr",
    null,
    "npm i -g @keelstackhq/bosun@<new> && bosun --version",
  ]);
  expect(update.bosun.guide).toBe("https://github.com/keelstackhq/bosun/blob/main/docs/update.md");
});

test.each(["hull", "bosun"] as const)("%s: every step, note and the guide are on the page", (k) => {
  const path = update[k];
  for (const s of path.steps) {
    for (const part of [s.cmd && `\`${s.cmd}\``, s.note, s.text])
      if (part) expect(page).toContain(part);
  }
  for (const n of path.notes) expect(page).toContain(n);
  expect(page).toContain(path.guide);
});

test("the home page shows only a short line, linking the full page", () => {
  expect(update.short.map((x) => x.cmd ?? x.text)).toEqual([
    "splice update --all",
    "bosun pin --latest --pr",
    "nothing to do",
  ]);
  expect(update.href).toBe("/docs/update/");
});

test("llms.txt links the update page; scenario D holds both one-line commands", () => {
  expect(llms).toContain("https://keelstack.dev/docs/update.md");
  const d = startScenarioMd("d");
  expect(d).toContain("splice@0.4.0 update --all");
  expect(d).toContain("bosun pin --latest --pr");
});
