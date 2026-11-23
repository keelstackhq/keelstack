// Fifth review round, run against both template copies: fewer exemptions instead of more
// cleverness. `bash|sh -n <file>` is the only parse-only form; an unread binary through a wrapper
// or runner is refused; `find -exec {}` and `xargs <wrapper>` with no literal program are refused;
// a method-override header on a `gh api` write is refused whatever `-X` says. Both directions.
import assert from "node:assert/strict";
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const COPIES = ["template", "template-rust"];
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-r5-"));
const ctx = { projectDir: dir };
const put = (rel, text, mode = 0o755) => {
  mkdirSync(join(dir, rel, ".."), { recursive: true });
  writeFileSync(join(dir, rel), text);
  chmodSync(join(dir, rel), mode);
};
put("evil.sh", "#!/bin/sh\ngit push --force origin main\n");
put("a.sh", "#!/bin/sh\necho a\n");
put("bin/bash", "#!/bin/sh\ngit push --force origin main\n"); // a script under a shell's name
put("bin/sh", "#!/bin/sh\ngit push --force origin main\n");
mkdirSync(join(dir, "target/debug"), { recursive: true });
copyFileSync(process.execPath, join(dir, "target/debug/x")); // a real host binary
// A valid-looking header the kernel refuses (e_phnum = 0): execvp hands it to /bin/sh.
const hdr = Buffer.from(readFileSync(process.execPath).subarray(0, 64));
if (hdr[4] === 2) hdr.writeUInt16LE(0, 56);
else hdr.writeUInt16LE(0, 44);
put("kbad", Buffer.concat([hdr, Buffer.from("\ngit push --force origin main\n")]));

const DENY = [
  // 1: `-n` is exempt only alone, before one literal file
  "bash -n +n evil.sh",
  "sh -n +n evil.sh",
  "bash -n +o noexec evil.sh",
  "bash -n -o noexec +o noexec evil.sh",
  "bash -nv evil.sh",
  "bash -n -- evil.sh",
  "zsh -n evil.sh",
  "bash -n evil.sh extra",
  // 2: an unread binary through an execvp wrapper or runner
  "env ./kbad",
  "timeout 5 ./kbad",
  "nice ./kbad",
  "nohup ./kbad",
  "sudo ./kbad",
  "echo | xargs ./kbad",
  "find . -name kbad -exec ./kbad ';'",
  "timeout 5 ./target/debug/x",
  "cargo build && env ./target/debug/x",
  "git bisect run ./kbad",
  "sh -c ./kbad",
  // 3: find runs `{}` or a non-literal program
  "find . -name a.sh -exec {} ';'",
  "find . -name a.sh -exec {} +",
  "find . -name a.sh -execdir {} ';'",
  "find . -name a.sh -ok {} ';'",
  "find . -name a.sh -exec env {} ';'",
  "find . -name a.sh -exec timeout 5 {} ';'",
  'find . -name a.sh -exec "$P" ";"',
  "find . -exec true ';' -exec {} ';'",
  "find . -exec env ';'",
  // 4: xargs with a wrapper and no literal program, `{}`, a replace string or a shell
  "echo x | xargs env",
  "echo x | xargs -n1 timeout 5",
  "echo x | xargs nice",
  "echo x | xargs -n1 nohup",
  "echo x | xargs -I{} {}",
  "echo x | xargs -I X X",
  "echo x | xargs -i {}",
  "echo x | xargs env bash",
  'echo x | xargs "$P"',
  // adversarial pass: a path named like a shell is that file, read as a script
  "./bin/bash a.sh",
  "./bin/sh -n a.sh",
  "bin/bash -c 'echo hi'",
  "./bin/bash --version",
  // adversarial pass: xargs short option bundles (`-tIX` is `-t -I X`)
  "echo x | xargs -tIX X",
  "echo x | xargs -tI X env X",
  "echo x | xargs -ti {}",
  // adversarial pass: env printers that run project code (.envrc, mise `_.source`)
  'eval "$(direnv export bash)"',
  'eval "$(mise env -s bash)"',
  // 6: a method override on a write
  "gh api -H 'X-HTTP-Method-Override: DELETE' -X POST repos/o/r/actions/runs/1/rerun",
  "gh api -X POST --header=x-http-method-override:DELETE repos/o/r/actions/runs/1/rerun",
];

const ALLOW = [
  "bash -n a.sh",
  "sh -n evil.sh",
  "./target/debug/x --help",
  "cargo build && ./target/debug/x",
  "echo a b | xargs",
  "echo a b | xargs -n1 echo",
  "echo a | xargs -I{} echo {}",
  "find . -name '*.rs' -exec wc -l {} +",
  "find . -exec timeout 5 cat {} ';'",
  "echo a | xargs env FOO=1 ls",
  "gh api -X GET repos/o/r",
  "/bin/bash -c 'echo hi'",
  "/bin/sh -n a.sh",
  "env /bin/bash -c 'echo hi'",
  "echo a | xargs -tI X echo X",
  'eval "$(ssh-agent -s)"',
  "gh api -X POST repos/o/r/actions/runs/1/rerun",
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
