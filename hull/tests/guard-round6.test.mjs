// Sixth review round, run against both template copies: a substitution's end is found by the
// same lexer that splits commands (escaped blanks and line continuations before `#`), sort/uniq/
// tree/less/file output options make later scripts unreadable, gh workflow run inputs read no
// file and carry no token, and a wildcard refspec cannot reach the default branch. Both directions.
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const COPIES = ["template", "template-rust"];
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-r6-"));
const ctx = { projectDir: dir };
writeFileSync(join(dir, "evil.sh"), "#!/bin/sh\ngit push --force origin main\n");
writeFileSync(join(dir, "ok.sh"), "#!/bin/sh\necho ok\n");

const DENY = [
  // 1: `#` after an escaped blank or a line continuation is a word's middle, not a comment
  'echo "$(echo a\\ #)" ; git push --force origin main\n)"',
  'echo "$(echo a\\\t#)" ; git push --force origin main\n)"',
  'echo "$(echo a\\\n#)" ; git push --force origin main\n)"',
  'echo "$(echo a\\ #)" ; npm publish\n)"',
  'echo "$(echo a\\\t#)" ; npm publish\n)"',
  'echo "$(echo a\\\n#)" ; npm publish\n)"',
  "echo $(echo a\\ #) ; git push --force origin main\n)",
  "git pu\\\nsh --force origin main",
  // review: a case pattern's `)`, `${...}` and `$((cmd) ...)` do not end a substitution early
  'echo "$(ca\\\nse x in x) git push --force origin main;; esac)"',
  'echo "$(time -p case x in x) git push --force origin main;; esac)"',
  'echo "$(coproc case x in x) git push --force origin main;; esac\nwait)"',
  'echo "$(function f case x in x) git push --force origin main;; esac\nf)"',
  'echo "$(case x in x|esac) git push --force origin main;; esac)"',
  'echo "$(echo ${x/)/}; git push --force origin main)"',
  'echo "$(echo ${x:-)}; git push --force origin main)"',
  'echo "$((true) #)\ngit push --force origin main\n)"',
  "if true; then case x in x) git push --force origin main;; esac; fi",
  "{ case x in x) git push --force origin main;; esac; }",
  // 2: sort/uniq/tree/less/file write a file the guard does not track
  "uniq evil.sh ok.sh; bash ok.sh",
  "sort -o ok.sh evil.sh && bash ok.sh",
  "sort evil.sh -o ok.sh; sh ok.sh",
  "sort -o ok.sh evil.sh & bash ok.sh",
  "sort --output=ok.sh evil.sh; bash ok.sh",
  "sort --out ok.sh evil.sh; bash ok.sh",
  "sort -ro ok.sh evil.sh; bash ok.sh",
  "tree -o ok.sh .; bash ok.sh",
  "less -o ok.sh evil.sh; bash ok.sh",
  "file -C -m ok.sh; bash ok.sh",
  "sort -S1 --compress-program=sh evil.sh",
  "sort --compress-program sh evil.sh",
  // 3: a file read into a workflow input
  "gh workflow run ci.yml -F x=@.env",
  "gh workflow run ci.yml --field x=@.env",
  "gh workflow run ci.yml --field=x=@.env",
  "gh workflow run ci.yml -Fx=@.env",
  "gh workflow run ci.yml --json < .env",
  "echo '{}' | gh workflow run ci.yml --json",
  // 4: a token-like variable sent as a dispatch input
  'gh api -X POST repos/o/r/actions/workflows/ci.yml/dispatches -f ref=x -f "inputs[x]=$GITHUB_TOKEN"',
  'gh workflow run ci.yml -f x="$TOKEN"',
  'gh workflow run ci.yml -f x="${NPM_SECRET}"',
  "gh workflow run ci.yml --raw-field=x=$API_KEY",
  // 5: a wildcard refspec that reaches the default branch
  "git push origin 'refs/heads/*:refs/heads/*'",
  "git push origin 'refs/heads/*'",
  "git push origin 'feat:refs/heads/ma*'",
];

const ALLOW = [
  'echo "$(echo a # )\n)"',
  'echo "$(echo a\\ b)"',
  "echo $(( 1 << 2 ))",
  "echo $(( (1 + 2) + 3 ))",
  'echo "$(echo ${x:-a}; echo b)"',
  'echo "$(case x in x) echo hi;; esac)"',
  "sort --compress-program=gzip evil.sh",
  'x=$(case a in a) echo hi;; esac); echo "$x"',
  "sort evil.sh; bash ok.sh",
  "sort -r evil.sh | uniq; bash ok.sh",
  "uniq evil.sh; bash ok.sh",
  "tree -L 2; bash ok.sh",
  "gh workflow run ci.yml -f x=1 --ref feat",
  "gh workflow run ci.yml -F n=3",
  "git push origin 'refs/heads/feat/*:refs/heads/feat/*'",
];

for (const copy of COPIES) {
  const { decide } = await import(`../${copy}/pieces/base/files/.claude/hooks/guard.mjs`);
  for (const c of DENY)
    test(`${copy} denied: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx)?.decision, "deny"));
  for (const c of ALLOW)
    test(`${copy} allowed: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx), null));
}
