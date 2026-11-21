import { expect, test } from "vite-plus/test";
import { facts } from "../data/facts";
import { legacy } from "../data/legacy";
import { bare, released, versions } from "../data/versions";
import { buildCommand, caption } from "./command";
import { NOT_OFFERED, OFFERED, pickerData as d } from "./picker-data";

const def = new Set(d.defaultOn);

test("pins what resolves: the workspace version once released, else the last old releases", () => {
  expect(d.spliceVersion).toBe(bare(versions.splice));
  expect(d.hullRef).toBe(versions.hull);
  if (released) {
    expect(d.spliceVersion).toBe(facts.version);
    expect(d.hullRef).toBe(facts.tag);
    expect(d.hullSource).toBe("gh:keelstackhq/keelstack//hull/template");
  } else {
    expect(d.spliceVersion).toBe(legacy.versions.splice);
    expect(d.hullRef).toBe(legacy.versions.hull);
    expect(d.hullSource).toBe("gh:keelstackhq/hull//template");
  }
  expect(d.splicePackage).toBe("@keelstackhq/splice");
});

test("defaults: the command run for the hero output", () => {
  expect(buildCommand(d, def)).toBe(
    `pnpm dlx @keelstackhq/splice@${d.spliceVersion} new \\\n` +
      `  ${d.hullSource} my-repo --ref ${d.hullRef} \\\n` +
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
    `pnpm dlx @keelstackhq/splice@${d.spliceVersion} new \\\n` +
      `  ${d.hullSource} my-repo --ref ${d.hullRef}`,
  );
});

test("no line ends in a space", () => {
  for (const on of [def, new Set<string>()]) {
    expect(buildCommand(d, on)).not.toMatch(/ \n| $/);
  }
});

test("caption names the pinned hull ref", () => {
  expect(caption(d)).toContain(versions.hull);
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
