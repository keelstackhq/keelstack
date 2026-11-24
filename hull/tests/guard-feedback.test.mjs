// Feedback from a consumer repo: the guard refused safe commands, so agents wrote scripts to a
// scratchpad and ran them with `bash`, which hid the commands from the guard. Both directions:
// the safe commands pass, and a script a command runs is checked like the command itself.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { decide, decideHead } from "../template/pieces/base/files/.claude/hooks/guard.mjs";

const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-feedback-"));
const scratch = mkdtempSync(join(tmpdir(), "guard-scratch-"));
const ctx = { projectDir: dir };
const put = (root, rel, text, mode = 0o644) => {
  mkdirSync(join(root, rel, ".."), { recursive: true });
  writeFileSync(join(root, rel), text);
  chmodSync(join(root, rel), mode);
};
put(dir, "scripts/x.sh", '#!/bin/bash\nif [ -f x ]; then grep -n "a[bc]{,]" x; fi\nls *.md\n');
put(dir, "scripts/ci-extra.sh", "#!/usr/bin/env bash\nset -eu\nbash scripts/x.sh\n", 0o755);
put(
  dir,
  "scripts/commented.sh",
  "#!/bin/bash\n# Prints `x`; never push or publish here.\ncommand -v x >/dev/null 2>&1\n2>/dev/null\n" +
    'git -C "$1" log -1\necho "$(date) ($(node --version 2>/dev/null || echo none))"\n',
);
put(dir, "bin/tool", readFileSync(process.execPath), 0o755); // a real host binary
put(dir, "scripts/py.py", "#!/usr/bin/env python3\nprint('see git push in docs')\n", 0o755);
const evil = {
  "force.sh": "#!/bin/bash\ngit push --force origin feat\n",
  "tag.sh": "git push --tags\n",
  "publish.sh": "#!/bin/sh\nset -e\ncd pkg && npm publish\n",
  "secret.sh": "#!/bin/bash\ncat .env\n",
  "hook.sh": "#!/bin/bash\necho x > .claude/hooks/guard.mjs\n",
  "default.sh": "#!/bin/bash\ngit push origin main\n",
  "nested.sh": `#!/bin/bash\nbash ${join(scratch, "force.sh")}\n`,
  "sourced.sh": `. ${join(scratch, "publish.sh")}\n`,
};
for (const [name, text] of Object.entries(evil)) put(scratch, name, text, 0o755);

for (const c of [
  // G1/G3: tests, regex, globs and braces in quotes are data
  "[ -f x ] && echo y",
  "[[ -n $X ]] || echo empty",
  'grep -n "a[bc]" f',
  "grep -E 'x{2,3}' f",
  'grep -n "{,]" f',
  "ls *.md docs/?.md",
  'echo "a*b"',
  "echo {a,b}",
  // G2: a heredoc that only names a script does not run it
  "python3 - <<'PY'\nopen('docs/a.md','w').write('see scripts/x.sh')\nPY",
  "echo scripts/x.sh",
  // G5: one rule whether a script runs through bash or directly
  "bash scripts/x.sh",
  "scripts/x.sh",
  "./scripts/x.sh",
  "bash scripts/ci-extra.sh",
  "./scripts/ci-extra.sh",
  "./bin/tool --help",
  "bash scripts/commented.sh 2>/dev/null",
  "scripts/py.py",
  // G6: fetch, then read
  "git fetch origin main && git log --oneline -1 origin/main",
  "git fetch && git status",
  // G4: dispatch and re-run of non-release workflows
  "gh workflow run bench.yml --ref feat",
  // (main's guard denies the gh api dispatch / re-run routes and a few others: no longer allowed,
  // see guard-no-regress.test.mjs LEGACY_DENIES_STAND)
  "gh run rerun 123 --failed",
  // a closed `$(...)` no longer swallows the rest of the line
  'git merge-base --is-ancestor "$sha" HEAD && git log -1 "$sha"',
  "diff <(ls a) <(ls b)",
]) {
  test(`safe: ${JSON.stringify(c)}`, () => assert.equal(decide(bash(c), ctx), null));
}

for (const c of [
  // the bypasses: a script run by a shell, by path or sourced is read like the command
  ...Object.keys(evil).flatMap((f) => [`bash ${join(scratch, f)}`]),
  `sh ${join(scratch, "tag.sh")}`,
  `${join(scratch, "force.sh")}`,
  `source ${join(scratch, "publish.sh")}`,
  `. ${join(scratch, "secret.sh")}`,
  `cd ${scratch} && ./force.sh`,
  `bash -e ${join(scratch, "force.sh")} arg`,
  // a script made earlier in the same command cannot be read before it runs
  "cat > /tmp/s.sh <<'EOF'\necho hi\nEOF\nbash /tmp/s.sh",
  "printf 'x' > s.sh; chmod +x s.sh; ./s.sh",
  // workflow writes that stay refused
  "gh api -X POST repos/o/r/actions/workflows/release.yml/dispatches -f ref=main",
  "gh api -X POST repos/o/r/actions/workflows/123/dispatches -f ref=main",
  "gh api -X POST repos/o/r/actions/workflows/$W/dispatches -f ref=main",
  "gh api -X PUT repos/o/r/actions/workflows/bench.yml/enable",
  "gh api -X DELETE repos/o/r/actions/runs/123",
  "gh api -X POST repos/o/r/actions/workflows/deploy.yml/dispatches repos/o/r/pulls/1/merge",
  "gh api -X POST repos/o/r/pulls/5/merge",
  "gh workflow run release.yml",
  // `$(...)` is parsed to its end now; what it hides stays refused
  'git push -u origin "$(git branch --show-current)"',
  'git push origin "$B"',
  'git push origin HEAD:"$B"',
  'git push "$R" feat',
  "git push origin `git branch --show-current`",
  'git -C "$D" push',
  'git tag "$T"',
  "echo $(date) && npm publish",
  'echo "$(npm publish)"',
  'echo "$(echo ")")" ; npm publish',
  "echo $(case x in a) echo;; esac) && git push --tags",
  "cat <(npm publish)",
  "x=$(cat .env)",
]) {
  test(`refused: ${JSON.stringify(c)}`, () => assert.equal(decide(bash(c), ctx)?.decision, "deny"));
}

test("a refused gh api write names the workflow alternatives", () => {
  // main's guard denies it first (its reason reaches the hook); guard.mjs's own rule names these
  const call = bash("gh api -X POST repos/o/r/actions/workflows/123/dispatches");
  assert.equal(decide(call, ctx)?.decision, "deny");
  const d = decideHead(call, ctx);
  assert.match(d.reason, /gh workflow run <file>/);
  assert.match(d.reason, /gh run rerun/);
});

test("a refusal inside a script names the script", () => {
  const d = decide(bash(`bash ${join(scratch, "nested.sh")}`), ctx);
  assert.match(d.reason, /force\.sh: git push --force/);
});

test("the template's own shell scripts pass when run", async () => {
  const { readdirSync } = await import("node:fs");
  const root = new URL("../template/pieces/", import.meta.url).pathname;
  const found = [];
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith(".sh")) found.push(p);
    }
  };
  walk(root);
  assert.ok(found.length > 0);
  for (const f of found) assert.equal(decide(bash(`bash ${f}`), ctx), null, f);
});
