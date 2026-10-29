// The release workflow (.github/workflows/release.yml) as text: a docspack release can start from
// the Actions tab (workflow_dispatch), which tags `docspack-v<version>` on main and publishes in the
// same run; a pushed tag still releases.
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

  it("the tag job runs only on a manual run, refuses off main or a taken tag, then pushes it", () => {
    const tag = job("tag");
    expect(tag).toContain("if: github.event_name == 'workflow_dispatch'");
    expect(tag).toContain("contents: write");
    expect(tag).toContain('"$REF" != "refs/heads/main"');
    expect(tag).toContain("packages/docspack/package.json");
    expect(tag).toContain('"$INPUT_VERSION" != "$version"');
    expect(tag).toContain('git ls-remote --exit-code --tags origin "refs/tags/$tag"');
    expect(tag).toContain('git push origin "refs/tags/$tag"');
  });

  it("plans and publishes the tag the run made, or the pushed tag", () => {
    const plan = job("plan");
    expect(plan).toContain("needs: tag");
    expect(plan).toContain("needs.tag.result == 'success' || needs.tag.result == 'skipped'");
    expect(plan).toContain("TAG: ${{ needs.tag.outputs.tag || github.ref_name }}");
    expect(plan).toContain('GITHUB_REF_NAME="$TAG" node .github/scripts/release-plan.mjs');
    expect(job("npm")).toContain("environment: release");
  });

  it("pins the actions of the tag job by commit SHA", () => {
    const uses = [...job("tag").matchAll(/uses: (\S+)/g)].map((m) => m[1]);
    expect(uses.length).toBeGreaterThan(0);
    for (const u of uses) expect(u).toMatch(/@[0-9a-f]{40}$/);
  });
});
