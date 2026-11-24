// Fourth review round, run against both template copies: `$(...)` bodies holding a comment or a
// heredoc (a `)` there does not close them), an ELF binary after a build, `${VAR:-}` PATH
// entries (venv activate), and runners that take a command (git rebase --exec, bisect run,
// submodule foreach, npm exec, at, expect -c, nix-shell --run). Both directions.
import assert from "node:assert/strict";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const COPIES = ["template", "template-rust"];
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });
const dir = mkdtempSync(join(tmpdir(), "guard-r4-"));
const ctx = { projectDir: dir };
const put = (rel, text, mode = 0o755) => {
  mkdirSync(join(dir, rel, ".."), { recursive: true });
  writeFileSync(join(dir, rel), text);
  chmodSync(join(dir, rel), mode);
};
put("scripts/ok.sh", "#!/bin/bash\necho ok\n");
put("scripts/gen.sh", "#!/bin/bash\necho gen\n");
mkdirSync(join(dir, "target/debug"), { recursive: true });
copyFileSync(process.execPath, join(dir, "target/debug/x")); // an ELF/Mach-O binary
copyFileSync("/bin/sh", join(dir, "target/debug/sh2")); // a shell under another name
put(
  ".venv/bin/activate",
  `deactivate () {
    if [ -n "\${_OLD_VIRTUAL_PATH:-}" ] ; then
        PATH="\${_OLD_VIRTUAL_PATH:-}"
        export PATH
        unset _OLD_VIRTUAL_PATH
    fi
    unset VIRTUAL_ENV
    if [ ! "\${1:-}" = "nondestructive" ] ; then
        unset -f deactivate
    fi
}
deactivate nondestructive
VIRTUAL_ENV=${dir}/.venv
export VIRTUAL_ENV
_OLD_VIRTUAL_PATH="$PATH"
PATH="$VIRTUAL_ENV/bin:$PATH"
export PATH
if [ -z "\${VIRTUAL_ENV_DISABLE_PROMPT:-}" ] ; then
    _OLD_VIRTUAL_PS1="\${PS1:-}"
    PS1="(.venv) \${PS1:-}"
    export PS1
fi
hash -r 2> /dev/null
`,
  0o644,
);
put(".venv/bin/pytest", "#!/bin/sh\ngit push --force origin main\n");
put("bad/env.sh", 'X=/safe\nread X\nPATH="$X:$PATH"\n', 0o644);
put("evil/pytest", "#!/bin/sh\ngit push --force origin main\n");
put("evil.sh", "#!/bin/sh\ngit push --force origin main\n");
// a text file with a fake magic number: the kernel refuses it and sh runs it (ENOEXEC)
put("fake-elf", "\x7fELF\ngit push --force origin main\n");
put(
  "fake-macho",
  Buffer.concat([
    Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
    Buffer.from("\ngit push --force origin main\n"),
  ]),
);
put(
  "fake-elf2",
  Buffer.concat([
    Buffer.from([0x7f, 0x45, 0x4c, 0x46, 2, 1, 1, 0, 0, 0]),
    Buffer.from("\ngit push --force origin main\n"),
  ]),
);

const DENY = [
  // MAJOR: a comment or heredoc inside "$(...)" does not end it
  'echo "$(echo # )\ngit push --force origin main\n)"',
  'echo "$(: # )\nnpm publish\n)"',
  "echo \"$(echo ')' # )\ncat .env\n)\"",
  'echo "$(cat <<X\n)\nX\ngit push --force origin main\n)"',
  'echo "$(cat <<-X\n)\n\tX\nnpm publish\n)"',
  "x=$(echo a #)\ngit push --force origin main\n)",
  // an opaque writer before a script: still refused (it may rewrite the script)
  "cargo build -p x && ./scripts/ok.sh",
  "pnpm install && bash scripts/gen.sh",
  "git pull && bash scripts/ok.sh",
  "git checkout main && bash scripts/ok.sh",
  "cargo build && ./target/debug/sh2 -c 'git push --force origin main'",
  // PATH entries from variables the guard cannot follow
  "source .venv/bin/activate && pytest -q",
  "source bad/env.sh",
  "PATH=${X:-$Y}:$PATH pytest",
  // fake binary headers are shell scripts (directly, through a wrapper, after a build)
  "./fake-elf",
  "timeout 5 ./fake-elf",
  "env ./fake-macho",
  "./fake-macho",
  "./fake-elf2 --help",
  "cargo build && ./fake-elf",
  // round 5: an unread binary through a wrapper is refused (it was allowed in round 4)
  "timeout 5 ./target/debug/x",
  // a tree writer can swap the binary for a script
  "git checkout x && ./target/debug/x",
  "tar xf a.tar && ./target/debug/x",
  // variables changed in ways the guard does not follow
  ': ${X:=./evil}; PATH="${X:-/usr/bin}:$PATH" pytest',
  "X=/usr/bin; : ${X=./evil}; PATH=$X:$PATH pytest",
  "X=/usr/bin; declare -n R=X; R=./evil; PATH=$X:$PATH pytest",
  "X=/usr/bin; ((X=1)); PATH=$X:$PATH pytest",
  "x=$( ((y=1<<2))\ngit push --force origin main\n2\n)",
  // runners
  "git rebase -ix 'git push --force origin main' HEAD~1",
  "git rebase --exe 'git push --force origin main' HEAD~1",
  "git rebase -xgit\\ push\\ --force\\ origin\\ main HEAD~1",
  'git bisect run sh -c "git push --force origin main"',
  "npm exec --prefix . -- git push --force origin main",
  "npm exec --prefix . git push --force origin main",
  "npm exec --frob git push --force origin main",
  "at -fevil.sh now",
  "at -mf evil.sh now",
  "git rebase --exec 'git push --force origin main' HEAD~1",
  'git rebase -x "npm publish" HEAD~2',
  'git rebase --exec="cat .env" HEAD~1',
  "git -C . rebase -i -x'npm publish' HEAD~1",
  "git bisect run git push --force origin main",
  "git submodule foreach --recursive 'git push --force origin main'",
  "npm exec -- git push --force origin main",
  "npm x -c 'npm publish'",
  "npm exec --call='cat .env'",
  "at now <<< 'git push --force origin main'",
  "at now <<EOF\nnpm publish\nEOF",
  "expect -c 'spawn git push --force origin main; interact'",
  "expect -c 'exec cat .env'",
  "nix-shell --run 'npm publish'",
  "nix-shell -p jq --command 'git push --force origin main'",
  "nix develop -c git push --force origin main",
];

const ALLOW = [
  'echo "$(git log -1 # the last commit\n)"',
  "echo $((1<<2))",
  "x=$(cat <<EOF\nhi )\nEOF\n); echo $x",
  "cargo build -p x && ./target/debug/x args",
  "./target/debug/x --help",
  "git rebase -s ort -X ours HEAD~1",
  "npm exec --yes -- vp --version",
  'git bisect run sh -c "cargo test"',
  "x=$( ((y=1<<2)); echo $y )",
  "cargo build && target/debug/x",
  "source .venv/bin/activate",
  "source .venv/bin/activate && python -V",
  "PATH=${X:-/usr/local/bin}:$PATH ls",
  "git rebase -x 'cargo test' HEAD~3",
  "git bisect run cargo test",
  "git submodule foreach git status",
  "npm exec -- vp --version",
  "nix-shell --run 'cargo test'",
  "nix develop -c cargo test",
  "at now <<< 'echo hi'",
  "at -l",
  "expect -c 'spawn ls; interact'",
];

for (const copy of COPIES) {
  const { decide, decideHead } = await import(
    `../${copy}/pieces/base/files/.claude/hooks/guard.mjs`
  );
  for (const c of DENY)
    test(`${copy} denied: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx)?.decision, "deny"));
  for (const c of ALLOW)
    test(`${copy} allowed: ${JSON.stringify(c).slice(0, 120)}`, () =>
      assert.equal(decide(bash(c), ctx), null));
  test(`${copy}: the "$(...)" comment repro is refused for the push, not as unreadable`, () =>
    assert.match(
      decideHead(bash('echo "$(echo # )\ngit push --force origin main\n)"'), ctx).reason,
      /rule force/,
    ));
}
