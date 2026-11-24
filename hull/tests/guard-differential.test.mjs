// Differential test of the no-regression rule at the hook level: the real entry (`node guard.mjs`,
// hook JSON on stdin, CLAUDE_PROJECT_DIR set) denies whatever main's guard denies for the same
// stdin, unless the Bash command is exactly an enumerated intended allow (`intendedAllow`), which
// head must then let through. main's guard is guard-legacy.mjs `judgeRaw` (main's hook body verbatim): run in process
// to pick the calls main denies, and through the same spawn path (fixtures/guard-diff/
// legacy-entry.mjs), where it must answer like main's original file (d90f917, when git has it). Probes: the shell probes of
// the PR #57, #60 and #66 review comments (fixtures/guard-diff/pr-probes.json), ELF binaries named
// like interpreters, writes to the files the guard protects, malformed stdin, and every call the
// other hull guard tests make (recorded by fixtures/guard-diff/hooks.mjs, replayed as raw stdin).
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const HULL = fileURLToPath(new URL("..", import.meta.url));
const MONOREPO = fileURLToPath(new URL("../..", import.meta.url));
const hooksOf = (copy) => join(HULL, copy, "pieces/base/files/.claude/hooks");
const LEGACY_ENTRY = join(HERE, "fixtures/guard-diff/legacy-entry.mjs");
const PROBES = JSON.parse(readFileSync(join(HERE, "fixtures/guard-diff/pr-probes.json"), "utf8"));
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });

/** A scratch git repo with the files the probes name, each holding a protected command, a
 * .splice/lock.toml and ELF binaries named like interpreters whose tail is a protected command. */
const scratch = mkdtempSync(join(tmpdir(), "guard-diff-"));
const BAD = {
  "bad.sh": "gh pr merge 1\n",
  "evil.sh": "#!/bin/sh\ngit push --force origin main\n",
  "x.sh": "git push --force origin main\n",
  "ok.sh": "echo ok\n",
  "m.sh": "gh workflow run release.yml\n",
  "nest.sh": "bash bad.sh\n",
  "tramp.sh": 'bash "$1"\n',
  "a.sh": "npm publish\n",
  f: "git push --tags\n",
  "s.sh": "git push --force origin main\n",
  "d/s.sh": "git push --force origin main\n",
  "sub/s/c.sh": "gh pr merge 1\n",
  "scripts/x.sh": "git push --force origin main\n",
  "bin/mytool": "#!/bin/sh\ngit push --force origin main\n",
  "r.py": 'import subprocess, sys\nsubprocess.run(["sh", sys.argv[1]])\n',
  "r.pl": 'system("sh", $ARGV[0]);\n',
  "pr.md": "body\n",
  "service-account.json": "{}\n",
  ".env": "TOKEN=x\n",
  ".claude/settings.json": "{}\n",
  ".claude/hooks/guard.mjs": "\n",
  ".github/workflows/release.yml": "on: push\n",
  ".bosun/approvers.toml": "\n",
};
for (const [rel, body] of Object.entries(BAD)) {
  mkdirSync(dirname(join(scratch, rel)), { recursive: true });
  writeFileSync(join(scratch, rel), body);
  chmodSync(join(scratch, rel), 0o755);
}
mkdirSync(join(scratch, ".splice"));
copyFileSync(join(HERE, "fixtures/lock/owned.toml"), join(scratch, ".splice/lock.toml"));
const ELVES = ["python3", "python", "perl", "node", "ruby", "bash", "sh", "true"];
for (const name of ELVES) {
  copyFileSync("/bin/true", join(scratch, name));
  writeFileSync(join(scratch, name), "\ngit push --force origin main\ngh pr merge 1\n", {
    flag: "a",
  });
  chmodSync(join(scratch, name), 0o755);
}
spawnSync("git", ["init", "-q", "-b", "main", scratch]);

const ELF_PROBES = ELVES.flatMap((n) => [
  `./${n}`,
  `sh ./${n}`,
  `bash ${n}`,
  `. ./${n}`,
  `source ${n}`,
  `cd . && ./${n}`,
  `exec ./${n}`,
  `timeout 5 ./${n}`,
  `env ./${n}`,
  `python3 r.py ./${n}`,
  `python3 -c 'import subprocess,sys; subprocess.run(["sh", sys.argv[1]])' ./${n}`,
  `perl -e 'system("sh", $ARGV[0])' ./${n}`,
  `perl r.pl ./${n}`,
  `node -e 'require("child_process").execFileSync("sh", [process.argv[1]])' ./${n}`,
  `xargs sh < ${n}`,
  `cat ${n} | sh`,
  `PATH=.:$PATH ${n}`,
]);

const PROTECTED = [
  ".claude/settings.json",
  ".claude/settings.local.json",
  ".claude/hooks/guard.mjs",
  ".claude/hooks/guard-legacy.mjs",
  ".claude/hooks/stop-checks.sh",
  ".github/workflows/release.yml",
  ".splice/lock.toml",
  ".bosun/approvers.toml",
  ".env",
  "apps/web/vite.config.ts",
  "apps/web/notes.ipynb",
];
const PATH_FORMS = (p) => [p, `a/../${p}`, `${scratch}/${p}`];
const TOOL_PROBES = PROTECTED.flatMap((p) =>
  PATH_FORMS(p).flatMap((file_path) => [
    { tool_name: "Write", tool_input: { file_path, content: "x" } },
    { tool_name: "Edit", tool_input: { file_path, old_string: "a", new_string: "b" } },
    { tool_name: "MultiEdit", tool_input: { file_path, edits: [] } },
    { tool_name: "NotebookEdit", tool_input: { notebook_path: file_path, new_source: "x" } },
    { tool_name: "Read", tool_input: { file_path } },
    bash(`echo x > ${file_path}`),
    bash(`sed -i s/a/b/ ${file_path}`),
    bash(`cp pr.md ${file_path}`),
  ]),
);

/** Raw stdin strings no JSON encoder makes: the hooks must judge the exact string. */
const RAW_PROBES = [
  "",
  "{not json",
  "null",
  "[]",
  '{"tool_name":"Bash","tool_input":{"command":"git push -f"',
  '{"tool_name":"Bash","tool_input":{"command":"x publish"',
  '{"tool_name":"Bash","tool_input":{"command":"ls"',
  '{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"},"tool_name":"Read"}',
  '{"tool_name":"Bash","tool_input":{"command":"gh pr merge 1","command":"ls"}}',
  '{"tool_name":"Bash","tool_input":{"command":"git push --force origin \\u006dain"}}',
  '{"tool_name":"Bash","tool_input":{"command":["git","push","--force"]}}',
  '{"tool_name":"Write","tool_input":{"file_path":".splice/lock.toml","file_path":"x"}}',
];

const PULLS = "gh api -X POST repos/o/r/pulls -f title=t -f head=feat -f base=main";
/** Enumerated intended allows: legacy denies, head must allow. */
const INTENDED = [
  PULLS,
  `${PULLS} -F body=@pr.md`,
  "gh api --method POST repos/o/r/pulls -f title=t -f head=feat -f base=main",
];

/** Spawns `node file [arg]` with `raw` on stdin; resolves to the hook's decision or null. */
function hook(args, raw, projectDir) {
  return new Promise((res, rej) => {
    const env = { ...process.env, CLAUDE_PROJECT_DIR: projectDir };
    delete env.NODE_OPTIONS;
    const cwd = existsSync(projectDir) ? projectDir : scratch;
    const p = spawn(process.execPath, args, { cwd, env });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.resume();
    p.on("error", rej);
    p.on("close", (code) => {
      if (code !== 0) return rej(new Error(`${args[0]} exited ${code} on ${raw.slice(0, 200)}`));
      res(out.trim() ? JSON.parse(out).hookSpecificOutput : null);
    });
    p.stdin.on("error", () => {});
    p.stdin.end(raw);
  });
}

/** Runs `fn` over `items` with a bounded number of child processes at once. */
async function pool(items, fn) {
  const out = Array.from({ length: items.length });
  let next = 0;
  // One child per CPU but one: the other test files (some timed) keep a CPU.
  const n = Math.max(1, Math.min(8, availableParallelism() - 1));
  await Promise.all(
    Array.from({ length: n }, async () => {
      while (next < items.length) {
        const i = next++;
        out[i] = await fn(items[i]);
      }
    }),
  );
  return out;
}

/** The original main guard (d90f917), when this checkout's git has it. */
const ORIGINAL = (() => {
  const r = spawnSync(
    "git",
    ["show", "d90f917:hull/template/pieces/base/files/.claude/hooks/guard.mjs"],
    { cwd: MONOREPO, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 },
  );
  if (r.status !== 0 || !r.stdout.includes("function main()")) return null;
  const file = join(mkdtempSync(join(tmpdir(), "guard-main-")), "guard.mjs");
  writeFileSync(file, r.stdout);
  return file;
})();

/** In process, main's hook body (`judgeRaw`) on `raw` with the hook's env: picks the cases main
 * denies, which are then judged by spawning both hooks. */
function legacyInProcess(judgeRaw, raw, projectDir) {
  const saved = process.env.CLAUDE_PROJECT_DIR;
  const write = process.stderr.write;
  process.env.CLAUDE_PROJECT_DIR = projectDir;
  process.stderr.write = () => true;
  try {
    return judgeRaw(raw);
  } finally {
    process.stderr.write = write;
    if (saved === undefined) delete process.env.CLAUDE_PROJECT_DIR;
    else process.env.CLAUDE_PROJECT_DIR = saved;
  }
}

/** Judges each case (`{ raw, projectDir, command }`) main's hook body denies with the real head
 * hook (spawned); returns the regressions and how many cases main's hook body denied. The spawned
 * legacy entry answers like the in-process `judgeRaw` and like main's original file (below). */
async function differential(copy, cases) {
  const guard = join(hooksOf(copy), "guard.mjs");
  const legacyFile = join(hooksOf(copy), "guard-legacy.mjs");
  const { intendedAllow } = await import(pathToFileURL(guard).href);
  const { judgeRaw } = await import(pathToFileURL(legacyFile).href);
  const denied = cases.filter(
    ({ raw, projectDir }) => legacyInProcess(judgeRaw, raw, projectDir)?.decision === "deny",
  );
  let legacyDenies = 0;
  const bad = [];
  await pool(denied, async ({ raw, projectDir, command }) => {
    const head = await hook([guard], raw, projectDir);
    legacyDenies++;
    const allow = command == null ? null : intendedAllow(command);
    if (allow === null && head?.permissionDecision !== "deny")
      bad.push(`${projectDir}: ${raw.slice(0, 300)}`);
  });
  return { bad, legacyDenies };
}

const probeCases = (projectDirs) =>
  projectDirs.flatMap((projectDir) => [
    ...[...PROBES, ...ELF_PROBES].map((command) => ({
      raw: JSON.stringify(bash(command)),
      projectDir,
      command,
    })),
    ...TOOL_PROBES.map((input) => ({
      raw: JSON.stringify(input),
      projectDir,
      command: input.tool_name === "Bash" ? input.tool_input.command : null,
    })),
    ...RAW_PROBES.map((raw) => ({ raw, projectDir, command: null })),
  ]);

test("the template-rust guard copies are the template's, byte for byte", () => {
  for (const f of ["guard.mjs", "guard-legacy.mjs"])
    assert.equal(
      readFileSync(join(hooksOf("template-rust"), f), "utf8"),
      readFileSync(join(hooksOf("template"), f), "utf8"),
      f,
    );
});

// template-rust's copies are byte-equal (above), so the hook runs judge the template's.
test("hook level, probes: head denies whatever main's guard denies", async () => {
  const { bad, legacyDenies } = await differential("template", probeCases([scratch, MONOREPO]));
  assert.deepEqual(bad, []);
  assert.ok(legacyDenies > 300, `only ${legacyDenies} legacy denies: probes lost?`);
});

test("hook level: the enumerated intended allows pass head although main's guard denies", async () => {
  const guard = join(hooksOf("template"), "guard.mjs");
  const legacy = [LEGACY_ENTRY, join(hooksOf("template"), "guard-legacy.mjs")];
  for (const command of INTENDED) {
    const raw = JSON.stringify(bash(command));
    assert.equal((await hook(legacy, raw, scratch))?.permissionDecision, "deny", command);
    assert.equal(await hook([guard], raw, scratch), null, command);
  }
});

test(
  "hook level: judgeRaw answers like main's original hook (d90f917)",
  { skip: !ORIGINAL },
  async () => {
    const legacy = [LEGACY_ENTRY, join(hooksOf("template"), "guard-legacy.mjs")];
    const cases = probeCases([scratch]).filter((_, i) => i % 6 === 0);
    const diff = [];
    await pool(cases, async ({ raw, projectDir }) => {
      const [a, b] = await Promise.all([
        hook([ORIGINAL], raw, projectDir),
        hook(legacy, raw, projectDir),
      ]);
      if (JSON.stringify(a) !== JSON.stringify(b)) diff.push(raw.slice(0, 200));
    });
    assert.deepEqual(diff, []);
  },
);

test("hook level, every call of the hull guard tests: head denies whatever main's guard denies", async () => {
  const out = join(mkdtempSync(join(tmpdir(), "guard-diff-out-")), "calls.jsonl");
  writeFileSync(out, "");
  const files = readdirSync(HERE)
    .filter((f) => /^guard-.*\.test\.mjs$/.test(f))
    .filter((f) => !["guard-differential.test.mjs", "guard-no-regress.test.mjs"].includes(f))
    .map((f) => join(HERE, f));
  assert.ok(files.length >= 10, `guard test files: ${files.length}`);
  const reg = pathToFileURL(join(HERE, "fixtures/guard-diff/register.mjs")).href;
  // NODE_OPTIONS reaches the per-file test processes; NODE_TEST_CONTEXT would make this run a
  // child of the outer runner.
  const env = { ...process.env, GUARD_DIFF_OUT: out, NODE_OPTIONS: `--import=${reg}` };
  delete env.NODE_TEST_CONTEXT;
  const r = spawnSync(process.execPath, ["--test", ...files], {
    env,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(r.status, 0, r.stdout.slice(-4000) + r.stderr.slice(-2000));
  const seen = new Set();
  const cases = [];
  for (const line of readFileSync(out, "utf8").split("\n").filter(Boolean)) {
    const { input, projectDir } = JSON.parse(line);
    const raw = JSON.stringify(input);
    const dir = projectDir ?? scratch;
    if (seen.has(`${dir}\0${raw}`)) continue;
    seen.add(`${dir}\0${raw}`);
    const command = input?.tool_name === "Bash" ? String(input?.tool_input?.command ?? "") : null;
    cases.push({ raw, projectDir: dir, command });
  }
  assert.ok(cases.length > 1000, `only ${cases.length} recorded calls: wrapper lost?`);
  const { bad, legacyDenies } = await differential("template", cases);
  assert.deepEqual(bad, []);
  assert.ok(legacyDenies > 300, `only ${legacyDenies} legacy denies`);
});
