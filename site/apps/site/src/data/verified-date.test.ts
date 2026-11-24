// agents/versions.json's `verified` date (#70 review): once the page follows a monorepo release,
// it is that release's date from bosun/CHANGELOG.md (`## X.Y.Z (YYYY-MM-DD)`), never the date the
// archived repos' pins were checked (legacy.ts).
import { expect, test } from "vite-plus/test";
import { facts, releaseDate } from "./facts";
import { legacy } from "./legacy";
import { verifiedOn } from "./versions";

test("releaseDate reads the dated heading of a version", () => {
  const log = "# Changelog\n\n## Unreleased\n\n## 0.22.1 (2026-11-24)\n\n## 0.22.0 (2026-11-23)\n";
  expect(releaseDate(log, "0.22.1")).toBe("2026-11-24");
  expect(releaseDate(log, "0.22.0")).toBe("2026-11-23");
  expect(releaseDate(log, "0.9.9")).toBeUndefined();
});

test("verified is the shown release's date when released, else legacy's", () => {
  if (facts.released) {
    expect(verifiedOn).toBe(facts.versionDate);
    expect(verifiedOn).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  } else {
    expect(verifiedOn).toBe(legacy.verifiedOn);
  }
});
