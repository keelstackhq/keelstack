import { expect, test } from "vite-plus/test";
import { facts } from "./facts";
import { org, roles, tools } from "./tools";
import { released, sources } from "./versions";

test("every tool in the monorepo has a card and every card a tool (a new tool fails here)", () => {
  expect(tools.map((t) => t.name).toSorted()).toEqual(facts.tools.map((t) => t.name).toSorted());
  expect(Object.keys(roles).toSorted()).toEqual(facts.tools.map((t) => t.name).toSorted());
});

test("cards link to the folder in the monorepo and show the one workspace version", () => {
  for (const t of tools) {
    expect(t.repo).toBe(`${facts.repoUrl}/tree/main/${t.name}`);
    expect(t.status).toBe(`v${facts.version}`);
  }
  expect(org.url).toBe("https://github.com/keelstackhq");
});

test("pitch is the crate description (hull: README lead), not typed copy", () => {
  for (const t of tools) {
    expect(t.pitch).toBe(facts.tools.find((f) => f.name === t.name)!.description);
  }
});

test("hoard shows the action pinned to what resolves (the workspace tag once released)", () => {
  expect(tools.find((t) => t.name === "hoard")!.detail).toContain(`uses: ${sources.hoardAction}`);
  if (released) {
    expect(sources.hoardAction).toBe(`keelstackhq/keelstack/hoard@v${facts.version}`);
  }
});

test("no copy uses the old repos or npm scope", () => {
  const text = JSON.stringify({ tools });
  expect(text).not.toMatch(/@keelstack\//);
  expect(text).not.toMatch(/github\.com\/keelstackhq\/(hull|splice|hoard|bosun|hoist|berth)\b/);
});
