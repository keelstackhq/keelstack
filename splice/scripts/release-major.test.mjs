// The Action's major tag (`uses: keelstackhq/keelstack/<tool>@v<major>`) is moved by job `major`
// of the root .github/workflows/release.yml (D-023), by a `git push` over SSH with the tag deploy
// key (the tags ruleset's only bypass actor). These tests run its step under bash against a real
// bare origin that stands in for git@github.com:o/r.git (url.insteadOf), and check the job.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { appendFileSync, chmodSync, existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gitEnv, job, releaseYml, runStep, stepScript } from "./release-yml.mjs";

const yml = releaseYml();
// The major tag is a step of job `release` (one tap: the registries are job `publish`).
const major = job(yml, "release");
const STEP = stepScript(major, "Move v<major> to the release commit");

/**
 * Run the step for `version` against a bare origin whose tags are {name: "NEW" | "OLD" |
 * {commit, annotated}}. `hijack`: after a push, a hook moves v<major> to commit HIJ (someone
 * moved it meanwhile); `fail`: "remote" (the origin is unreachable) or "push" (the origin refuses
 * every push, as the ruleset would). Every ref update the origin accepts is a write.
 */
function run(version, tags, { hijack = false, fail = "" } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "release-major-"));
  const env = gitEnv(dir);
  const g = (cwd, ...args) => execFileSync("git", args, { cwd, env, encoding: "utf8" }).trim();
  const origin = join(dir, "origin.git");
  const seed = join(dir, "seed");
  g(dir, "init", "-q", "--bare", origin);
  g(dir, "init", "-q", seed);
  const sha = {};
  for (const name of ["OLD", "HIJ", "NEW"]) {
    g(seed, "commit", "-q", "--allow-empty", "-m", name);
    sha[name] = g(seed, "rev-parse", "HEAD");
  }
  for (const [name, v] of Object.entries(tags)) {
    const t = typeof v === "string" ? { commit: v, annotated: false } : v;
    if (t.annotated) g(seed, "tag", "-a", name, "-m", name, sha[t.commit]);
    else g(seed, "tag", name, sha[t.commit]);
  }
  // Every commit reaches the origin (as on GitHub: the release commits are reachable from tags).
  g(seed, "push", "-q", origin, ...Object.values(sha).map((s, i) => `${s}:refs/heads/c${i}`), "--tags");
  const log = join(dir, "writes");
  writeFileSync(log, "");
  const hook = (name, body) => {
    const f = join(origin, "hooks", name);
    writeFileSync(f, `#!/usr/bin/env bash\n${body}\n`);
    chmodSync(f, 0o755);
  };
  if (fail === "push") hook("pre-receive", "echo 'refusing: ruleset' >&2; exit 1");
  hook(
    "post-receive",
    `while read -r old new ref; do echo "$old $new $ref" >>"${log}"; done` +
      (hijack ? `\ngit update-ref "refs/tags/v${version.split(".")[0]}" ${sha.HIJ}` : ""),
  );
  const target = fail === "remote" ? join(dir, "missing.git") : origin;
  appendFileSync(env.GIT_CONFIG_GLOBAL, `[url "${target}"]\n\tinsteadOf = git@github.com:o/r.git\n`);
  const r = runStep(STEP, {
    cwd: dir,
    env: { ...env, VERSION: version, TAG: `v${version}`, GH_REPO: "o/r", RUNNER_TEMP: dir },
  });
  const now = {};
  for (const line of g(origin, "for-each-ref", "--format=%(refname:strip=2) %(*objectname)%(objectname)", "refs/tags")
    .split("\n")
    .filter(Boolean)) {
    const [name, both] = line.split(" ");
    now[name] = Object.keys(sha).find((k) => both.startsWith(sha[k])) ?? both;
  }
  const writes = readFileSync(log, "utf8")
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      const [, n, ref] = l.split(" ");
      return { to: Object.keys(sha).find((k) => sha[k] === n) ?? n, ref };
    });
  return { ...r, sha, tags: now, writes, dir };
}

test("the major tag is v<major> of the release", () => {
  for (const [version, name] of [
    ["0.22.0", "v0"],
    ["1.2.0", "v1"],
    ["2.0.0", "v2"],
  ]) {
    const r = run(version, { [`v${version}`]: "NEW" });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.tags[name], "NEW", version);
    assert.match(r.stdout, new RegExp(`${name} -> v${version}`));
  }
});

test("a new stable release moves a stale major to its commit (annotated tag peeled)", () => {
  const r = run("0.22.1", { "v0.22.0": "OLD", "v0.22.1": { commit: "NEW", annotated: true }, v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW", "the release commit, not the tag object");
  assert.equal(r.writes.length, 1);
  const [push] = r.writes;
  assert.equal(push.ref, "refs/tags/v0");
  assert.equal(push.to, "NEW", "a forced move to the commit");
  assert.match(r.stdout, new RegExp(`v0 -> v0\\.22\\.1 \\(${r.sha.NEW}\\)`));
});

test("a missing major tag is created", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW");
  assert.deepEqual(r.writes, [{ to: "NEW", ref: "refs/tags/v0" }]);
});

test("a major already on the commit (a re-run) stays there", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", v0: "NEW" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW");
  assert.ok(
    r.writes.every((w) => w.to === "NEW"),
    "it never points elsewhere",
  );
});

test("re-releasing an older version never moves the major backwards", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", "v0.23.0": "OLD", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /newest stable v0\.\* tag is v0\.23\.0, not v0\.22\.1: v0 not moved/);
  assert.equal(r.tags.v0, "OLD");
  assert.deepEqual(r.writes, []);
});

test("a newer prerelease does not count: the stable release still moves the major", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", "v0.23.0-rc.1": "OLD", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW");
});

test("a newer release of another major does not count either", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", "v1.0.0": "OLD", v1: "OLD", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW");
  assert.equal(r.tags.v1, "OLD", "v1 is untouched");
});

test("newest means version order, not text order", () => {
  const up = run("0.10.0", { "v0.9.0": "OLD", "v0.10.0": "NEW", v0: "OLD" });
  assert.equal(up.status, 0, up.stderr);
  assert.equal(up.tags.v0, "NEW");
  const back = run("0.9.0", { "v0.9.0": "NEW", "v0.10.0": "OLD", v0: "OLD" });
  assert.equal(back.status, 0, back.stderr);
  assert.equal(back.tags.v0, "OLD");
});

test("tags that are not vX.Y.Z do not count as newer", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", "v0.99": "OLD", "v0.99.0.1": "OLD", "v0.x.y": "OLD", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0, "NEW");
});

test("the completeness check needs the major tag on the release commit", () => {
  // Someone moves v0 between the push and the check.
  const r = run("0.22.1", { "v0.22.1": "NEW", v0: "OLD" }, { hijack: true });
  assert.equal(r.status, 1);
  assert.match(r.stdout, new RegExp(`::error::v0 is at ${r.sha.HIJ}, not at ${r.sha.NEW}`));
  assert.doesNotMatch(r.stdout, /v0 -> /);
});

test("a failing remote or a refused push fails the job, nothing moves", () => {
  for (const fail of ["remote", "push"]) {
    const r = run("0.22.1", { "v0.22.1": "NEW", v0: "OLD" }, { fail });
    assert.notEqual(r.status, 0, fail);
    assert.deepEqual(r.writes, [], fail);
    assert.equal(r.tags.v0, "OLD", fail);
  }
});

test("the step leaves no repository or key behind in RUNNER_TEMP", () => {
  const r = run("0.22.1", { "v0.22.1": "NEW", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(!existsSync(join(r.dir, "major-tag")), "the scratch repository is removed");
});

test("release.yml moves the major tag only after the release, crates.io and npm", () => {
  assert.match(major, /needs: \[plan, draft, publish\]/);
  assert.match(major, /needs\.publish\.result == 'success'/);
  // Held back while crates.io or npm skipped a package that is not published yet.
  const gate = "needs.publish.outputs.crates_skipped == '' && needs.publish.outputs.npm_skipped == ''";
  const at = major.indexOf("      - name: Move v<major> to the release commit");
  assert.ok(major.slice(at, at + 400).includes(gate), "the move step is gated on nothing being skipped");
  assert.ok(
    major.includes(`      - name: Load the tag deploy key\n        if: ${gate}`),
    "the key is not even loaded when the major tag is held",
  );
  assert.match(major, /github\.event_name == 'workflow_dispatch' && needs\.plan\.outputs\.release == 'true'/);
  assert.match(major, /\n    environment: release-tags\n/);
  assert.match(major, /TAG: \$\{\{ needs\.plan\.outputs\.tag \}\}/);
  assert.doesNotMatch(major, /github\.ref_name|github\.event\./);
  assert.match(major, /RELEASE_TAG_KEY: \$\{\{ secrets\.RELEASE_TAG_KEY \}\}/);
});

test("the major step moves a tag only: no branch, one forced tag push over SSH", () => {
  const step = major.slice(major.indexOf("      - name: Move v<major> to the release commit"));
  const stepText = step.slice(0, step.indexOf("      - name: Stop the ssh-agent"));
  assert.doesNotMatch(stepText, /actions\/checkout|refs\/heads|gh api/);
  assert.match(STEP, /remote="git@github\.com:\$GH_REPO\.git"/);
  const pushes = STEP.split("\n").filter((l) => !/^\s*#/.test(l) && /\bgit\b.*\bpush\b/.test(l));
  assert.deepEqual(
    pushes.map((l) => l.trim()),
    ['git -C "$work" push -q "$remote" "+refs/tags/$major"'],
  );
  const r = run("0.22.1", { "v0.22.1": "NEW", v0: "OLD" });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(
    r.writes.every((w) => w.ref === "refs/tags/v0"),
    JSON.stringify(r.writes),
  );
});

test("a prerelease never moves the major: the plan releases only an exact X.Y.Z", () => {
  const re = new RegExp(job(yml, "plan").match(/re='([^']+)'/)[1]);
  assert.ok(re.test("0.22.1"));
  assert.ok(!re.test("0.23.0-rc.1"));
  assert.ok(!re.test("1"));
  assert.ok(!re.test("01.2.3"));
});
