// Differential test of the no-regression rule: guard.mjs (head) denies every call the legacy judge
// (guard-legacy.mjs, main's guard at d90f917) denies, unless the command is exactly one of the
// enumerated intended allows (`intendedAllow`). Probes: every call the other hull guard tests make
// (re-run with `decide` wrapped, in their own fixtures) and the shell probes of the PR #57, #60 and
// #66 review comments (fixtures/guard-diff/pr-probes.json), in a scratch repo and in this checkout.
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";

const COPIES = ["template", "template-rust"];
const HERE = fileURLToPath(new URL(".", import.meta.url));
const MONOREPO = fileURLToPath(new URL("../..", import.meta.url));
const PROBES = JSON.parse(readFileSync(join(HERE, "fixtures/guard-diff/pr-probes.json"), "utf8"));
const bash = (command) => ({ tool_name: "Bash", tool_input: { command } });

/** A scratch repo with the files the probes name, each holding a protected command. */
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
  "pr.md": "body\n",
  "service-account.json": "{}\n",
  ".env": "TOKEN=x\n",
};
for (const [rel, body] of Object.entries(BAD)) {
  mkdirSync(dirname(join(scratch, rel)), { recursive: true });
  writeFileSync(join(scratch, rel), body);
  chmodSync(join(scratch, rel), 0o755);
}

for (const copy of COPIES) {
  const hooks = `../${copy}/pieces/base/files/.claude/hooks`;
  const { decide, intendedAllow } = await import(`${hooks}/guard.mjs`);
  const { decideLegacy, failClosedLegacy } = await import(`${hooks}/guard-legacy.mjs`);
  const legacy = (input, ctx) => {
    try {
      return decideLegacy(input, ctx);
    } catch {
      return failClosedLegacy(JSON.stringify(input)) ? { decision: "deny" } : null;
    }
  };

  test(`${copy}: PR review probes: head denies whatever legacy denies (bar the allow list)`, () => {
    const bad = [];
    let legacyDenies = 0;
    for (const projectDir of [scratch, MONOREPO])
      for (const cmd of PROBES) {
        const ctx = { projectDir };
        if (legacy(bash(cmd), ctx)?.decision !== "deny") continue;
        legacyDenies++;
        if (decide(bash(cmd), ctx)?.decision !== "deny" && intendedAllow(cmd) === null)
          bad.push(`${projectDir}: ${cmd}`);
      }
    assert.deepEqual(bad, []);
    assert.ok(legacyDenies > 50, `only ${legacyDenies} legacy denies: probes lost?`);
  });

  test(`${copy}: an enumerated allow is never something guard.mjs itself denies for a reason`, () => {
    // guard.mjs runs first: its own denies (credentials, taint) apply inside the allow shapes.
    const ctx = { projectDir: scratch };
    const cmd =
      "gh api -X POST repos/o/r/pulls -f title=t -f head=b -f base=main -F body=@service-account.json";
    assert.equal(intendedAllow(cmd), "gh-api-pulls");
    assert.equal(decide(bash(cmd), ctx)?.decision, "deny");
  });
}

test("every call of the hull guard tests: head denies whatever legacy denies (bar the allow list)", () => {
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
  const calls = readFileSync(out, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  assert.ok(calls.length > 1000, `only ${calls.length} recorded calls: wrapper lost?`);
  assert.ok(calls.filter((c) => c.legacy === "deny").length > 300);
  assert.deepEqual(
    calls.filter((c) => !c.ok).map((c) => c.command ?? c.tool),
    [],
  );
});
