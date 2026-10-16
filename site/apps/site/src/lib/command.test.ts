import { expect, test } from "vite-plus/test";
import { DEFAULT_ON, PIECES, buildCommand } from "./command";

const def = new Set(DEFAULT_ON);

test("today, defaults", () => {
  expect(buildCommand(def, "today")).toBe(
    "node scripts/render.mjs new --template template --out ../my-repo \\\n" +
      "  --piece base --piece preact-app:web --piece hono-worker:api --data scope=acme\n" +
      "cd ../my-repo && git init && pnpm install && pnpm run ci",
  );
});

test("splice, defaults", () => {
  expect(buildCommand(def, "splice")).toBe(
    "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo \\\n" +
      "  --piece preact-app:web --piece hono-worker:api",
  );
});

test("keeps PIECES order whatever the insertion order", () => {
  expect(buildCommand(new Set(["docs", "web"]), "splice")).toContain(
    "--piece preact-app:web --piece docs:docs",
  );
});

test("today, nothing selected", () => {
  expect(buildCommand(new Set(), "today")).toContain("  --piece base --data scope=acme\n");
});

test("splice, nothing selected: no dangling backslash", () => {
  expect(buildCommand(new Set(), "splice")).toBe(
    "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo",
  );
});

test("no line ends in a space", () => {
  for (const mode of ["today", "splice"] as const) {
    for (const on of [def, new Set<string>()]) {
      expect(buildCommand(on, mode)).not.toMatch(/ \n| $/);
    }
  }
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
