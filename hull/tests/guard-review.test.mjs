// Review of the script-reading guard: the gh api endpoint is the first positional, -F @file reads
// files, scripts the guard cannot fully read are refused (fail closed), comments are found by the
// tokenizer, a nested script shares the chain, stdin scripts, shebang forms, trampolines,
// variables, PATH, find -exec, aliases and a broader release-workflow match. Both directions.
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { decide } from "../template/pieces/base/files/.claude/hooks/guard.mjs";

const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-review-"));
const S = mkdtempSync(join(tmpdir(), "guard-review-s-"));
const ctx = { projectDir: dir };
const put = (root, rel, text, mode = 0o755) => {
  mkdirSync(join(root, rel, ".."), { recursive: true });
  writeFileSync(join(root, rel), text);
  chmodSync(join(root, rel), mode);
};
const force = "git push --force origin feat\n";
put(S, "force.sh", `#!/bin/bash\n${force}`);
put(S, "ok.sh", "#!/bin/bash\necho ok\n");
put(S, "big.sh", `#!/bin/bash\n${"echo x\n".repeat(80 * 1024)}${force}`);
put(S, "big-ok.sh", `#!/bin/bash\n${"echo x\n".repeat(80 * 1024)}`);
put(S, "nul.sh", `#!/bin/bash\necho a\0b\n${force}`);
put(S, "elf", Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0, 0, 0]));
put(S, "backtick.sh", `#!/bin/bash\nx="\n# \`git push --force origin feat\`\n"\n`);
put(S, "quoted-hash.sh", `#!/bin/bash\nbash -c "echo '\n#' ; git push --force origin feat"\n`);
put(S, "comment-ok.sh", "#!/bin/bash\n# never push or publish here\necho ok # or release\n");
put(S, "later.sh", `#!/bin/bash\nbash ${S}/made.sh\n`);
put(S, "missing.sh", `#!/bin/bash\nbash ${S}/does-not-exist.sh\n`);
put(S, "envS.sh", `#!/usr/bin/env -S bash -e\n${force}`);
put(S, "busybox.sh", `#!/bin/busybox sh\n${force}`);
put(S, "tramp.sh", '#!/bin/bash\nbash "$1"\n');
put(S, "tramp2.sh", '#!/bin/bash\n"$@"\n');
put(S, "tramp3.sh", '#!/bin/bash\ngit push "$@"\n');
put(S, "varpath.sh", '#!/bin/bash\nF=/x/f.sh\nbash "$F"\n');
put(S, "bin/evil.sh", `#!/bin/bash\n${force}`);
put(S, "bin/fine", "#!/bin/bash\necho fine\n");
put(S, "deep/0.sh", "echo leaf\n");
for (let n = 1; n <= 40; n++) put(S, `deep/${n}.sh`, `bash ${S}/deep/${n - 1}.sh\n`);
put(S, "fn.sh", "#!/bin/bash\nf() { git push --force origin main; }\nf\n");
put(S, "pbin/evilsrc", `#!/bin/bash\n${force}`);
put(
  S,
  "cargo-env",
  '#!/bin/sh\ncase ":${PATH}:" in\n    *:"$HOME/.cargo/bin":*)\n        ;;\n    *)\n        export PATH="$HOME/.cargo/bin:$PATH"\n        ;;\nesac\n',
);
put(
  S,
  "case-evil.sh",
  'case "$x" in\n  a) ;;\nesac\ncase y in\n  *) bash ' + S + "/force.sh ;;\nesac\n",
);
put(dir, "scripts/run.sh", "#!/bin/bash\nbash scripts/ok.sh\n");
put(dir, "scripts/ok.sh", "#!/bin/bash\necho ok\n");

const DENY = [
  // BLOCKER: the endpoint is the first positional, not any arg that looks like one
  "gh api -X POST repositories/123/git/refs -f ref=refs/tags/v9 -f sha=abc --jq repos/o/r/actions/workflows/ci.yml/dispatches",
  "gh api -X POST repositories/123/merges -f base=main -f head=feat --template repos/o/r/actions/runs/1/rerun",
  "gh api -X POST user/keys -f key=x --template repos/o/r/actions/runs/1/rerun",
  "gh api -X POST orgs/o/hooks --jq repos/o/r/actions/runs/1/rerun",
  "gh api -X POST repositories/123/actions/workflows/ci.yml/dispatches -f ref=main",
  // -F @file reads a local file into the request
  "gh api -X POST repos/o/r/actions/workflows/ci.yml/dispatches -f ref=main -F 'inputs[x]=@.env'",
  "gh api -X POST repos/o/r/actions/runs/1/rerun -F x=@.env",
  "gh api -X GET repos/o/r/issues -F q=@.env",
  "gh api -X GET repos/o/r/issues --field=q=@notes.txt",
  "gh api -X POST repos/o/r/actions/runs/1/rerun --input body.json",
  // scripts the guard cannot fully read
  `bash ${S}/big.sh`,
  `bash ${S}/big-ok.sh`,
  `bash ${S}/nul.sh`,
  `${S}/nul.sh`,
  `bash ${S}/elf`,
  `bash ${S}/deep/40.sh`,
  // comments are found by the tokenizer
  `bash ${S}/backtick.sh`,
  `bash ${S}/quoted-hash.sh`,
  // a nested script shares the chain; a missing nested script is refused
  `printf 'git push --force origin feat' > ${S}/made.sh; bash ${S}/later.sh`,
  `bash ${S}/missing.sh`,
  // stdin scripts
  `bash < ${S}/force.sh`,
  `bash <${S}/force.sh`,
  `cat ${S}/force.sh | bash`,
  `cat ${S}/ok.sh | bash -s`,
  `bash <<< "$(cat ${S}/force.sh)"`,
  `bash <<< "$C"`,
  `C='git push --force'; bash <<EOF\n$C\nEOF`,
  "xargs bash < /dev/null",
  `source /dev/stdin < ${S}/force.sh`,
  `bash <(cat ${S}/force.sh)`,
  // shebang forms
  `${S}/envS.sh`,
  `${S}/busybox.sh`,
  // trampolines and variables
  `bash ${S}/tramp.sh ${S}/force.sh`,
  `bash ${S}/tramp2.sh ${S}/force.sh`,
  `bash ${S}/tramp3.sh --force origin feat`,
  `bash ${S}/varpath.sh`,
  `F=${S}/force.sh; bash $F`,
  `bash "$F"`,
  `bash ${S}/*.sh`,
  `"$D/force.sh"`,
  `PATH=${S}/bin:$PATH evil.sh`,
  `export PATH=${S}/bin:$PATH; evil.sh`,
  "PATH=$X:$PATH evil.sh",
  `find ${S} -name force.sh -exec bash {} +`,
  `find ${S} -name force.sh -exec sh {} ';'`,
  `find ${S} -name force.sh | xargs bash`,
  "alias p='git push --force'; p origin x",
  "alias g=git",
  "echo ls | sh",
  // second review round: functions, -s, fds, startup files, source via PATH
  "f() { git push --force origin main; }; f",
  "function f { git push --force origin main; }; f",
  "f() ( git push --force origin main ); f",
  "x() { gh release create v1; }; x",
  `bash ${S}/fn.sh`,
  `bash -s x < ${S}/force.sh`,
  `bash -x -s a b < ${S}/force.sh`,
  `bash /dev/fd/3 3<${S}/force.sh`,
  `bash /proc/self/fd/3 3<${S}/force.sh`,
  `BASH_ENV=${S}/force.sh bash -c true`,
  `env BASH_ENV=${S}/force.sh bash ${S}/ok.sh`,
  `export ENV=${S}/force.sh; sh -c true`,
  `bash --rcfile ${S}/force.sh -i -c true`,
  `bash --init-file=${S}/force.sh -i`,
  `PATH=${S}/pbin:$PATH; source evilsrc`,
  `bash ${S}/case-evil.sh`,
  "PATH=$VIRTUAL_ENV/bin:$PATH pytest",
  "gh api -X POST repos/o/r/actions/runs/1/rerun -H 'X-HTTP-Method-Override: DELETE'",
  "gh api -X GET repos/o/r/issues/1 -H 'X-HTTP-Method-Override: DELETE'",
  // release workflows: release, publish, deploy by default
  "gh api -X POST repos/o/r/actions/workflows/publish.yml/dispatches -f ref=main",
  "gh api -X POST repos/o/r/actions/workflows/deploy.yml/dispatches -f ref=main",
  "gh workflow run publish.yml",
  "gh workflow run deploy.yml --ref feat",
];

const ALLOW = [
  "gh api -X POST repos/o/r/actions/workflows/ci.yml/dispatches -f ref=main --jq .x",
  "gh api -X POST /repos/o/r/actions/runs/1/rerun",
  "gh api repos/o/r/pulls --jq '.[].title'",
  "gh api -X GET repos/o/r/issues -f q=x",
  "gh api -X GET repos/o/r/issues -F per_page=100",
  "gh api -X POST repos/o/r/actions/workflows/bench.yml/dispatches -f ref=feat -f 'inputs[x]=@notes'",
  "gh workflow run bench.yml --ref feat",
  `bash ${S}/ok.sh`,
  `bash ${S}/comment-ok.sh`,
  `bash < ${S}/ok.sh`,
  `${S}/elf --help`,
  `PATH=${S}/bin:$PATH fine`,
  'export PATH="$HOME/.cargo/bin:$PATH"; cargo --version',
  `bash ${S}/tramp.sh ${S}/ok.sh`,
  `bash ${S}/deep/10.sh`,
  "bash scripts/run.sh",
  "bash --version",
  `find ${S} -name '*.sh' -exec ls -l {} +`,
  `find ${S} -name '*.sh' -exec sh -c 'echo "$1"' _ {} ';'`,
  "bash <<'EOF'\necho $HOME\nEOF",
  `. ${S}/cargo-env && cargo --version`,
  'export PATH="$PWD/node_modules/.bin:$PATH"; vp --version',
  'export PATH="$(pwd)/bin:$PATH"; ls',
  "g() { git status; }; g",
  "ENV=production node -e 1",
  "BASH_ENV= bash -c true",
  `bash -s < ${S}/ok.sh`,
];

for (const c of DENY)
  test(`refused: ${JSON.stringify(c).slice(0, 140)}`, () =>
    assert.equal(decide(bash(c), ctx)?.decision, "deny"));
for (const c of ALLOW)
  test(`allowed: ${JSON.stringify(c).slice(0, 140)}`, () =>
    assert.equal(decide(bash(c), ctx), null));

test("-F @file names -f as the alternative", () => {
  const d = decide(bash("gh api -X GET repos/o/r/issues -F q=@.env"), ctx);
  assert.match(d.reason, /-f\/--raw-field/);
});

test("an unreadable script names the safe alternative", () => {
  const d = decide(bash(`bash ${S}/big-ok.sh`), ctx);
  assert.match(d.reason, /rule unscanned/);
  assert.match(d.reason, /bash <path>/);
});

test("GUARD_RELEASE_WORKFLOWS narrows or widens the release-workflow match", () => {
  const prev = process.env.GUARD_RELEASE_WORKFLOWS;
  try {
    process.env.GUARD_RELEASE_WORKFLOWS = "release|ship";
    assert.equal(decide(bash("gh workflow run deploy.yml --ref feat"), ctx), null);
    assert.equal(decide(bash("gh workflow run ship.yml --ref feat"), ctx)?.decision, "deny");
    assert.equal(
      decide(bash("gh api -X POST repos/o/r/actions/workflows/ship.yml/dispatches"), ctx)?.decision,
      "deny",
    );
  } finally {
    if (prev === undefined) delete process.env.GUARD_RELEASE_WORKFLOWS;
    else process.env.GUARD_RELEASE_WORKFLOWS = prev;
  }
});
