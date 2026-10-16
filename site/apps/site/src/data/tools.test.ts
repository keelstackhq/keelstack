import { expect, test } from "vite-plus/test";
import { fitSteps, org, tools } from "./tools";

test("lists hull, splice and hoard in that order", () => {
  expect(tools.map((t) => t.name)).toEqual(["hull", "splice", "hoard"]);
});

test("every tool links to its repo in the keelstackhq org (D-001)", () => {
  for (const tool of tools) {
    expect(tool.repo).toBe(`${org.url}/${tool.name}`);
  }
  expect(org.url).toBe("https://github.com/keelstackhq");
});

test("every tool has a summary, at least two facts and a status", () => {
  for (const tool of tools) {
    expect(tool.summary.length).toBeGreaterThan(0);
    expect(tool.facts.length).toBeGreaterThanOrEqual(2);
    expect(tool.status.length).toBeGreaterThan(0);
  }
});

test("no copy uses the old keelstack org or npm scope", () => {
  const text = JSON.stringify({ tools, fitSteps });
  expect(text).not.toMatch(/@keelstack\//);
  expect(text).not.toMatch(/github\.com\/keelstack\//);
  expect(text).not.toMatch(/gh:keelstack\//);
});

test("the fit steps go template, bootstrap and update, cached CI", () => {
  expect(fitSteps.map((s) => s.tool)).toEqual(["hull", "splice", "hoard"]);
});
