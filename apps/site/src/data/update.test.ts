import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { update } from "./content";
import { buildLlmsTxt } from "../lib/llms";

const page = readFileSync(new URL("../pages/docs/update.md", import.meta.url), "utf8");

test("the hull update path is the same on the page, the home block and llms.txt", () => {
  expect(update.steps.map((s) => s.cmd)).toEqual([
    "git switch -c chore/splice-update",
    "splice check --json",
    "splice diff",
    "splice update --all",
    "git grep -n '<<<<<<<'",
    "splice check",
    "pnpm install && pnpm run ci",
    'git add -A && git commit -m "chore: splice update to hull vX.Y.Z"',
  ]);
  const llms = buildLlmsTxt();
  for (const s of update.steps) {
    expect(page).toContain(`\`${s.cmd}\``);
    expect(llms).toContain(`\`${s.cmd}\``);
    if (s.note) {
      expect(page).toContain(s.note);
      expect(llms).toContain(s.note);
    }
  }
  expect(page).toContain(update.guide);
  expect(llms).toContain(update.guide);
});
