import { expect, test } from "vite-plus/test";
import { facts } from "../data/facts";
import { buildCommand, caption } from "./command";
import { NOT_OFFERED, OFFERED, pickerData as d } from "./picker-data";

const def = new Set(d.defaultOn);

test("pins the workspace version for splice and hull", () => {
  expect(d.spliceVersion).toBe(facts.version);
  expect(d.hullRef).toBe(`v${facts.version}`);
  expect(d.hullSource).toBe("gh:keelstackhq/keelstack//hull/template");
  expect(d.splicePackage).toBe("@keelstackhq/splice");
});

test("defaults: the command run for the hero output", () => {
  expect(buildCommand(d, def)).toBe(
    `pnpm dlx @keelstackhq/splice@${facts.version} new \\\n` +
      `  gh:keelstackhq/keelstack//hull/template my-repo --ref v${facts.version} \\\n` +
      "  --piece preact-app:web --piece hono-worker:api",
  );
});

test("keeps piece order whatever the insertion order", () => {
  expect(buildCommand(d, new Set(["docs", "web"]))).toContain(
    "--piece preact-app:web --piece docs:docs",
  );
});

test("nothing selected: no dangling backslash", () => {
  expect(buildCommand(d, new Set())).toBe(
    `pnpm dlx @keelstackhq/splice@${facts.version} new \\\n` +
      `  gh:keelstackhq/keelstack//hull/template my-repo --ref v${facts.version}`,
  );
});

test("no line ends in a space", () => {
  for (const on of [def, new Set<string>()]) {
    expect(buildCommand(d, on)).not.toMatch(/ \n| $/);
  }
});

test("caption names the pinned hull ref", () => {
  expect(caption(d)).toContain(`v${facts.version}`);
  expect(caption(d)).toContain("--ref");
});

test("offered pieces and their instance names come from hull's piece.toml", () => {
  expect(d.pieces.map((p) => `${p.piece}:${p.name}`)).toEqual([
    "preact-app:web",
    "hono-worker:api",
    "ui-storybook:ui",
    "astro-site:site",
    "rust-tool:tool",
    "tauri-app:desktop",
    "docs:docs",
  ]);
});

test("every hull piece is offered or listed as not offered (a new piece fails here)", () => {
  const ids = facts.pieces.filter((p) => p.template === "template").map((p) => p.id);
  expect([...OFFERED, ...NOT_OFFERED].toSorted()).toEqual(ids.toSorted());
});
