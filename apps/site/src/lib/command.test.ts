import { expect, test } from "vite-plus/test";
import { CAPTION, DEFAULT_ON, HULL_REF, PIECES, SPLICE_VERSION, buildCommand } from "./command";

const def = new Set(DEFAULT_ON);

test("pins the released splice and hull versions", () => {
  expect(SPLICE_VERSION).toBe("0.2.0");
  expect(HULL_REF).toBe("v0.1.1");
});

test("defaults: the command run for the hero output", () => {
  expect(buildCommand(def)).toBe(
    "pnpm dlx @keelstackhq/splice@0.2.0 new \\\n" +
      "  gh:keelstackhq/hull//template my-repo --ref v0.1.1 \\\n" +
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
    "pnpm dlx @keelstackhq/splice@0.2.0 new \\\n" +
      "  gh:keelstackhq/hull//template my-repo --ref v0.1.1",
  );
});

test("no line ends in a space", () => {
  for (const on of [def, new Set<string>()]) {
    expect(buildCommand(on)).not.toMatch(/ \n| $/);
  }
});

test("caption names the pinned versions", () => {
  expect(CAPTION).toContain("v0.1.1");
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
