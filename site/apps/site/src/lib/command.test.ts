import { expect, test } from "vite-plus/test";
import { versions } from "../data/versions";
import { CAPTION, DEFAULT_ON, HULL_REF, PIECES, SPLICE_VERSION, buildCommand } from "./command";

const def = new Set(DEFAULT_ON);

test("pins the released splice and hull versions", () => {
  expect(SPLICE_VERSION).toBe(versions.splice);
  expect(HULL_REF).toBe(versions.hull);
});

test("defaults: the command run for the hero output", () => {
  expect(buildCommand(def)).toBe(
    `pnpm dlx @keelstackhq/splice@${versions.splice} new \\\n` +
      `  gh:keelstackhq/hull//template my-repo --ref ${versions.hull} \\\n` +
      "  --piece preact-app:web --piece hono-worker:api",
  );
});

test("keeps PIECES order whatever the insertion order", () => {
  expect(buildCommand(new Set(["docs", "web"]))).toContain(
    "--piece preact-app:web --piece docs:docs",
  );
});

test("nothing selected: no dangling backslash", () => {
  expect(buildCommand(new Set())).toBe(
    `pnpm dlx @keelstackhq/splice@${versions.splice} new \\\n` +
      `  gh:keelstackhq/hull//template my-repo --ref ${versions.hull}`,
  );
});

test("no line ends in a space", () => {
  for (const on of [def, new Set<string>()]) {
    expect(buildCommand(on)).not.toMatch(/ \n| $/);
  }
});

test("caption names the pinned versions", () => {
  expect(CAPTION).toContain(versions.hull);
  expect(CAPTION).toContain("--ref");
});

test("PIECES lists the seven pieces", () => {
  expect(PIECES.map((p) => `${p.piece}:${p.name}`)).toEqual([
    "preact-app:web",
    "hono-worker:api",
    "ui-storybook:ui",
    "astro-site:site",
    "rust-tool:cli",
    "tauri-app:desktop",
    "docs:docs",
  ]);
});
