// #64 and #46: agents open PRs through `gh api` (the only GitHub route in cloud sessions) while
// merge, auto-merge, tags and releases stay refused; a substitution in a PR body or title is data;
// quoted `<>`/`>` is not a redirect; `find <guarded dir> -delete` is a write. Both template copies.
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import * as main from "../template/pieces/base/files/.claude/hooks/guard.mjs";
import * as rust from "../template-rust/pieces/base/files/.claude/hooks/guard.mjs";

const dir = mkdtempSync(join(tmpdir(), "guard-prs-"));
mkdirSync(join(dir, ".claude/hooks"), { recursive: true });
mkdirSync(join(dir, "build"), { recursive: true });
writeFileSync(join(dir, ".claude/hooks/guard.mjs"), "// x\n");
writeFileSync(join(dir, "body.md"), "Closes #1\n");
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });

const ALLOWED = [
  "gh api -X POST repos/o/r/pulls -f title=t -f head=feat -f base=main -f body=x",
  "gh api repos/o/r/pulls -f title=t -f head=feat -f base=main",
  "gh api --method POST /repos/o/r/pulls -f title=t -f head=feat -f base=main -f draft=true",
  'gh api -X POST repos/o/r/pulls -f title=t -f head=feat -f base=main -f body="$(cat body.md)"',
  'gh api -X POST repos/o/r/pulls -f title=t -f head=feat -f base=main --raw-field=body="$(cat body.md)"',
  'gh pr create --title t --body "$(cat body.md)"',
  'gh pr create -t "fix: $(echo x)" -b "$(cat body.md)"',
  "gh api -X GET repos/o/r/pulls",
  // #46: quoted `<>` and `>` are data, not redirects
  "grep '<>' .claude/hooks/guard.mjs",
  "grep '>' .claude/hooks/guard.mjs",
  'grep -c "<" .claude/hooks/guard.mjs',
  // find that only reads, or writes outside guarded dirs
  "find . -name '*.md'",
  "find build -delete",
  "find .claude -name '*.mjs' -exec cat {} ;",
];

const REFUSED = [
  ["gh api -X PUT repos/o/r/pulls/1/merge", "ghApi"],
  ["gh api -X POST repos/o/r/pulls/1/merge", "ghApi"],
  ["gh api -X PUT repos/o/r/pulls/1/ccr/auto_merge -f merge_method=squash", "ghApi"],
  ["gh api -X PATCH repos/o/r/pulls/1 -f state=closed", "ghApi"],
  ["gh api -X POST repos/o/r/pulls/1/reviews -f event=APPROVE", "ghApi"],
  ["gh api -X POST repos/o/r/../../pulls -f title=t", "ghApi"],
  ["gh api -X POST 'repos/o/r/pulls?x=1' -f title=t", "ghApi"],
  [
    "gh api graphql -f query='mutation { enablePullRequestAutoMerge(input: {}) { clientMutationId } }'",
    "ghApi",
  ],
  ["gh pr merge 1", "ghMerge"],
  ["gh pr merge 1 --auto --admin", "ghMerge"],
  ["gh api -X POST repos/o/r/git/refs -f ref=refs/tags/v1 -f sha=abc", "ghApi"],
  ["gh api -X POST repos/o/r/releases -f tag_name=v1", "ghApi"],
  ["gh release create v1", "publish"],
  ["git push origin v1.0.0:refs/tags/v1.0.0", "tag"],
  ["git push origin HEAD:main", "pushDefault"],
  ["git push --force origin feat", "force"],
  ["gh api -X POST repos/o/r/actions/workflows/release.yml/dispatches -f ref=main", "ghApi"],
  ["gh api -X POST repos/o/r/pulls -F body=@body.md", "ghField"],
  ["gh api -X POST repos/o/r/pulls --input body.md", "ghApi"],
  ['gh api -X POST repos/o/r/pulls -f title=t -f body="$(cat .env)"', "secret"],
  ['gh api -X POST repos/o/r/pulls -f title=t -f body="$GITHUB_TOKEN"', "secret"],
  ['gh pr create --title t --body "$(git push origin main)"', "pushDefault"],
  // a run-time word anywhere but a -f / --body value is still not what the guard read
  ['gh api -X POST "repos/o/r/$(echo pulls/1/merge)" -f a=b', "evasion|ghApi|ghMerge|ghField"],
  ['gh api -X "$(echo PUT)" repos/o/r/pulls/1/merge', "evasion|ghApi|ghMerge|ghField"],
  ['gh api -X POST repos/o/r/pulls -F body="$(echo @.env)"', "evasion|ghApi|ghMerge|ghField"],
  ['gh api -X POST repos/o/r/pulls -f "$(echo k)=v"', "evasion|ghApi|ghMerge|ghField"],
  ['gh api -X POST -H -f "x=$(echo y)"', "evasion|ghApi|ghMerge|ghField"],
  ['gh "$(echo pr)" merge 1', "evasion|ghApi|ghMerge|ghField"],
  ['gh pr merge "$(echo 1)" --body "$(echo x)"', "ghMerge"],
  // #46: a real redirect into a guarded file stays refused
  ["echo x >'.claude/hooks/guard.mjs'", "protected"],
  ["echo x '>'>.claude/hooks/guard.mjs", "protected"],
  // #46: find that deletes or runs a writer under a guarded dir
  ["find . -delete", "protected"],
  ["find -delete", "protected"],
  ["find .claude -delete", "protected"],
  ["find -L .claude/hooks -name x -delete", "protected"],
  ["find .claude/hooks -exec rm {} +", "protected"],
  ["find . -name x -execdir rm {} ;", "protected"],
];

for (const [name, g] of [
  ["template", main],
  ["template-rust", rust],
]) {
  test(`guard prs (${name}): PR creation and data words pass`, async () => {
    for (const c of ALLOWED) {
      const r = await g.decide(bash(c), { projectDir: dir });
      assert.equal(r, null, `${c}: ${JSON.stringify(r)}`);
    }
  });

  test(`guard prs (${name}): merge, auto-merge, tags, releases and hidden words stay refused`, async () => {
    for (const [c, rule] of REFUSED) {
      const r = await g.decide(bash(c), { projectDir: dir });
      assert.equal(r?.decision, "deny", `${c}: ${JSON.stringify(r)}`);
      assert.match(r.reason, new RegExp(`\\(rule (${rule})\\b`), `${c}: ${r.reason}`);
    }
  });

  test(`guard prs (${name}): the gh api refusal names the PR route`, async () => {
    const r = await g.decide(bash("gh api -X PUT repos/o/r/pulls/1/merge"), { projectDir: dir });
    assert.match(r.reason, /gh api -X POST repos\/<owner>\/<repo>\/pulls/);
  });
}

test("proxy-ca: the test-only PROXY_CA_UPDATE counts only with PROXY_CA_STORE", () => {
  const sh = readFileSync(
    new URL("../template-rust/pieces/base/files/scripts/proxy-ca.sh", import.meta.url),
    "utf8",
  );
  const uses = sh.split("\n").filter((l) => l.includes("PROXY_CA_UPDATE") && !/^\s*#/.test(l));
  assert.ok(uses.length > 0);
  for (const l of uses) assert.match(l, /PROXY_CA_STORE/, l);
});
