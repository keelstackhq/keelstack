// The release workflow (.github/workflows/release.yml) as text: a docspack release can start from
// the Actions tab (workflow_dispatch), whose `tag` job only creates `docspack-v<version>` on main
// with the release App's token; that tag's push run publishes, as a hand-pushed tag does.
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";

const yml = readFileSync(
  new URL("../../../.github/workflows/release.yml", import.meta.url),
  "utf8",
);
const job = (name: string): string => {
  const start = yml.indexOf(`\n  ${name}:\n`);
  expect(start).toBeGreaterThan(-1);
  const next = yml.slice(start + 1).search(/\n {2}[a-z-]+:\n/);
  return next === -1 ? yml.slice(start) : yml.slice(start, start + 1 + next);
};

describe("release workflow", () => {
  it("keeps the tag-push trigger and adds a manual run with an optional version", () => {
    expect(yml).toContain('  push:\n    tags: ["*-v[0-9]*"]');
    expect(yml).toMatch(/workflow_dispatch:\n\s+inputs:\n\s+version:[\s\S]*?required: false/);
  });

  it("the tag job runs only on a manual run, in a main-only environment, with the release App", () => {
    const tag = job("tag");
    expect(tag).toContain("if: github.event_name == 'workflow_dispatch'");
    expect(tag).toContain("environment: release-tag");
    expect(tag).toContain("permissions:\n      contents: read");
    expect(tag).not.toMatch(/^\s+contents: write/m);
    expect(tag).toContain("client-id: ${{ vars.HOIST_RELEASE_APP_CLIENT_ID }}");
    expect(tag).toContain("private-key: ${{ secrets.HOIST_RELEASE_APP_KEY }}");
    expect(tag).toContain("GH_TOKEN: ${{ steps.app.outputs.token }}");
    expect(tag).not.toContain("github.token");
  });

  it("refuses off main, a wrong version or a taken tag, then creates the tag on main's head", () => {
    const tag = job("tag");
    expect(tag).toContain('"$REF" != "refs/heads/main"');
    expect(tag).toContain("packages/docspack/package.json");
    expect(tag).toContain('"$INPUT_VERSION" != "$version"');
    expect(tag).toContain('gh api "repos/$GITHUB_REPOSITORY/git/ref/tags/$TAG"');
    expect(tag).toContain('-f "ref=refs/tags/$TAG" -f "sha=$GITHUB_SHA"');
  });

  it("publishes only from a tag push, behind the release environment", () => {
    expect(job("plan")).toContain("if: github.event_name == 'push'");
    expect(job("npm")).toContain("environment: release");
  });

  it("pins every action by commit SHA", () => {
    const uses = [...yml.matchAll(/uses: (\S+)/g)].map((m) => m[1]);
    expect(uses.length).toBeGreaterThan(0);
    for (const u of uses) expect(u).toMatch(/@[0-9a-f]{40}$/);
  });
});
