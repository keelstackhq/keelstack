// Seventh review round, run against both template copies: in a push, publish, merge, tag, release
// or secrets command a word that changes at run time (`$(`, backtick, `${`, `$'`, `$"`, a brace
// expansion, a line continuation) is refused; `gh workflow run <id>` is refused; a segment's
// substitutions are read after the earlier `cd` and `PATH=` of the line. Both directions.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const COPIES = ["template", "template-rust"];
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-r7-"));
const ctx = { projectDir: dir };
const force = "#!/bin/sh\ngit push --force origin main\n";
mkdirSync(join(dir, "d"));
mkdirSync(join(dir, "bin"));
for (const f of ["d/s.sh", "bin/mytool", "bin/ssh-agent"]) {
  writeFileSync(join(dir, f), force);
  chmodSync(join(dir, f), 0o755);
}
writeFileSync(join(dir, "s.sh"), "#!/bin/sh\necho ok\n");

const DENY = [
  // 1: a word that changes at run time in a risky command
  "npm publish$()",
  "cargo publish$(true)",
  "git push --force$() origin x",
  "git push -f$() origin x",
  "git push --tags$()",
  "gh pr merge$() 1",
  "gh workflow run releas$()e.yml",
  "npm publish\\\n$()",
  "npm publish${x}",
  "npm publish`true`",
  "npm $'publish'",
  'npm $"publish"',
  'echo "`echo \\`git push --force origin main\\``"',
  "git push origin HEAD:{main,x}",
  "git push origin HEAD:ma{i,}n",
  "git push origin {HEAD:main,x}",
  "git push origin HEAD:refs/heads/{main,x}",
  "git push origin HEAD:refs/heads/{a..b}",
  // 2: a workflow id does not name its workflow
  "gh workflow run 12345",
  "gh workflow run 12345 --ref feat",
  "gh workflow run --ref feat 12345",
  // 3: a substitution runs after the earlier cd / PATH
  'cd d && echo "$(./s.sh)"',
  "cd d && x=$(bash s.sh)",
  "cd d; cat <(sh s.sh)",
  'PATH=./bin:$PATH; x="$(mytool)"',
  'PATH=./bin:$PATH; eval "$(ssh-agent)"',
];

const ALLOW = [
  "git log --format=%H",
  'echo "$(date)"',
  "cargo build",
  "gh pr view 1 --json title",
  "gh workflow run ci.yml --ref feat",
  'echo "$(./s.sh)"',
  "git push -u origin feat",
  // (main's guard denies the gh api dispatch / re-run routes and a few others: no longer allowed,
  // see guard-no-regress.test.mjs LEGACY_DENIES_STAND)
  "echo {a,b}",
  "git log --format='{a,b}'",
];

for (const copy of COPIES) {
  const { decide } = await import(`../${copy}/pieces/base/files/.claude/hooks/guard.mjs`);
  for (const c of DENY)
    test(`${copy} denied: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx)?.decision, "deny"));
  for (const c of ALLOW)
    test(`${copy} allowed: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx), null));
  test(`${copy} a pushed branch from $(...) keeps the literal-branch fix`, () => {
    const d = decide(bash('git push -u origin "$(git branch --show-current)"'), ctx);
    assert.equal(d?.decision, "deny");
    assert.match(d.reason, /write the branch name literally/);
  });
  test(`${copy} a changed word says to write it plainly`, () =>
    assert.match(decide(bash("npm publish$()"), ctx)?.reason ?? "", /write the word plainly/));
}
