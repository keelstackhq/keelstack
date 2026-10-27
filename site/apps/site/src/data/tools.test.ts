import { expect, test } from "vite-plus/test";
import { org, tools } from "./tools";

test("lists all six tools, the original three first", () => {
  expect(tools.map((t) => t.name)).toEqual(["hull", "splice", "hoard", "bosun", "hoist", "berth"]);
});

test("every tool links to its repo in the keelstackhq org (D-001)", () => {
  for (const tool of tools) {
    expect(tool.repo).toBe(`${org.url}/${tool.name}`);
  }
  expect(org.url).toBe("https://github.com/keelstackhq");
});

test("statuses and styles follow the design", () => {
  expect(tools.map((t) => t.status)).toEqual([
    "v0.3.0",
    "v0.2.1",
    "v0.3.0",
    "v0.2.3",
    "v0.2.0",
    "v0.1.0",
  ]);
  expect(tools.map((t) => t.statusStyle)).toEqual([
    "filled",
    "outline",
    "dashed",
    "outline",
    "outline",
    "outline",
  ]);
  expect(tools.map((t) => t.detailMono)).toEqual([false, true, true, true, true, true]);
});

test("hoard detail is the pinned Action from its README", () => {
  expect(tools[2]?.detail).toBe(
    "uses: keelstackhq/hoard@v0 · GitHub Actions cache or your own R2 bucket",
  );
});

test("no copy uses the old keelstack org or npm scope", () => {
  const text = JSON.stringify({ tools });
  expect(text).not.toMatch(/@keelstack\//);
  expect(text).not.toMatch(/github\.com\/keelstack\//);
  expect(text).not.toMatch(/gh:keelstack\//);
});
