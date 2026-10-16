import { expect, test } from "vite-plus/test";
import { org, tools } from "./tools";

test("lists hull, splice and hoard in that order", () => {
  expect(tools.map((t) => t.name)).toEqual(["hull", "splice", "hoard"]);
});

test("every tool links to its repo in the keelstackhq org (D-001)", () => {
  for (const tool of tools) {
    expect(tool.repo).toBe(`${org.url}/${tool.name}`);
  }
  expect(org.url).toBe("https://github.com/keelstackhq");
});

test("statuses and styles follow the design", () => {
  expect(tools.map((t) => t.status)).toEqual(["pre-0.1", "under construction", "not released yet"]);
  expect(tools.map((t) => t.statusStyle)).toEqual(["filled", "outline", "dashed"]);
  expect(tools.map((t) => t.detailMono)).toEqual([false, true, false]);
});

test("no copy uses the old keelstack org or npm scope", () => {
  const text = JSON.stringify({ tools });
  expect(text).not.toMatch(/@keelstack\//);
  expect(text).not.toMatch(/github\.com\/keelstack\//);
  expect(text).not.toMatch(/gh:keelstack\//);
});
