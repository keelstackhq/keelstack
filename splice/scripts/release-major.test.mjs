// The Action's major tag (`uses: keelstackhq/keelstack/<tool>@v<major>`) is moved by job `major`
// of the root .github/workflows/release.yml (D-023), through the GitHub API. These tests run its
// step under bash against a fake `gh` (./release-yml.mjs) and check the job's structure.
import test from "node:test";
import assert from "node:assert/strict";
import { fakeGh, job, releaseYml, runStep, stepScript } from "./release-yml.mjs";

const NEW = "0123456789abcdef0123456789abcdef01234567";
const OLD = "fedcba9876543210fedcba9876543210fedcba98";
const OBJ = "1111111111111111111111111111111111111111";
const HIJ = "3333333333333333333333333333333333333333";

const yml = releaseYml();
const major = job(yml, "major");
const STEP = stepScript(major, "Move v<major> to the release commit");

/** Run the step for `version` with the remote's tags {name: commit | {commit, object}}. */
function run(version, tags, extra = {}) {
  const t = {};
  for (const [name, v] of Object.entries(tags)) t[name] = typeof v === "string" ? { commit: v, object: v } : v;
  const fake = fakeGh({ tags: t, ...extra });
  const r = runStep(STEP, {
    cwd: fake.dir,
    env: { ...fake.env, VERSION: version, TAG: `v${version}`, GH_TOKEN: "dummy", GH_REPO: "o/r" },
  });
  const writes = fake.calls().filter((c) => c.includes("PATCH") || c.includes("POST"));
  return { ...r, tags: fake.state().tags, writes };
}

test("the major tag is v<major> of the release", () => {
  for (const [version, name] of [
    ["0.22.0", "v0"],
    ["1.2.0", "v1"],
    ["2.0.0", "v2"],
  ]) {
    const r = run(version, { [`v${version}`]: NEW });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.tags[name]?.commit, NEW, version);
    assert.match(r.stdout, new RegExp(`${name} -> v${version}`));
  }
});

test("a new stable release moves a stale major to its commit (annotated tag peeled)", () => {
  const r = run("0.22.1", { "v0.22.0": OLD, "v0.22.1": { commit: NEW, object: OBJ }, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW, "the release commit, not the tag object");
  assert.equal(r.writes.length, 1);
  const [patch] = r.writes;
  assert.ok(patch.includes("repos/o/r/git/refs/tags/v0"), patch.join(" "));
  assert.ok(patch.includes(`sha=${NEW}`) && patch.includes("force=true"), patch.join(" "));
  assert.match(r.stdout, new RegExp(`v0 -> v0\\.22\\.1 \\(${NEW}\\)`));
});

test("a missing major tag is created", () => {
  const r = run("0.22.1", { "v0.22.1": NEW });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW);
  assert.ok(r.writes[0].includes("POST") && r.writes[0].includes("ref=refs/tags/v0"), r.writes[0].join(" "));
});

test("a major already on the commit (a re-run) stays there", () => {
  const r = run("0.22.1", { "v0.22.1": NEW, v0: NEW });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW);
  assert.ok(r.writes.every((c) => c.includes(`sha=${NEW}`)), "it never points elsewhere");
});

test("re-releasing an older version never moves the major backwards", () => {
  const r = run("0.22.1", { "v0.22.1": NEW, "v0.23.0": OLD, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /newest stable v0\.\* tag is v0\.23\.0, not v0\.22\.1: v0 not moved/);
  assert.equal(r.tags.v0.commit, OLD);
  assert.deepEqual(r.writes, []);
});

test("a newer prerelease does not count: the stable release still moves the major", () => {
  const r = run("0.22.1", { "v0.22.1": NEW, "v0.23.0-rc.1": OLD, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW);
});

test("a newer release of another major does not count either", () => {
  const r = run("0.22.1", { "v0.22.1": NEW, "v1.0.0": OLD, v1: OLD, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW);
  assert.equal(r.tags.v1.commit, OLD, "v1 is untouched");
});

test("newest means version order, not text order", () => {
  const up = run("0.10.0", { "v0.9.0": OLD, "v0.10.0": NEW, v0: OLD });
  assert.equal(up.status, 0, up.stderr);
  assert.equal(up.tags.v0.commit, NEW);
  const back = run("0.9.0", { "v0.9.0": NEW, "v0.10.0": OLD, v0: OLD });
  assert.equal(back.status, 0, back.stderr);
  assert.equal(back.tags.v0.commit, OLD);
});

test("tags that are not vX.Y.Z do not count as newer", () => {
  const r = run("0.22.1", { "v0.22.1": NEW, "v0.99": OLD, "v0.99.0.1": OLD, "v0.x.y": OLD, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.tags.v0.commit, NEW);
});

test("the completeness check needs the major tag on the release commit", () => {
  // Someone moves v0 between the PATCH and the check.
  const r = run("0.22.1", { "v0.22.1": NEW, v0: OLD }, { hijack: HIJ });
  assert.equal(r.status, 1);
  assert.match(r.stdout, new RegExp(`::error::v0 is at ${HIJ}, not at ${NEW}`));
  assert.doesNotMatch(r.stdout, /v0 -> /);
});

test("a failing API call fails the job before anything moves", () => {
  for (const fail of ["git/matching-refs", "commits/"]) {
    const r = run("0.22.1", { "v0.22.1": NEW, v0: OLD }, { fail });
    assert.notEqual(r.status, 0, fail);
    assert.deepEqual(r.writes, [], fail);
  }
});

test("release.yml moves the major tag only after the release, crates.io and npm", () => {
  assert.match(major, /needs: \[plan, release, crates, npm\]/);
  assert.match(major, /needs\.npm\.result == 'success'/);
  assert.match(major, /needs\.crates\.result == 'success'/);
  assert.match(major, /needs\.plan\.outputs\.released == 'true' \|\| needs\.release\.result == 'success'/);
  assert.match(major, /github\.event_name == 'workflow_dispatch' && needs\.plan\.outputs\.release == 'true'/);
  assert.match(major, /\n    environment: release\n/);
  assert.match(major, /\n    permissions:\n      contents: write\n    env:/);
  assert.match(major, /TAG: \$\{\{ needs\.plan\.outputs\.tag \}\}/);
  assert.doesNotMatch(major, /github\.ref_name|github\.event\.|inputs\./);
});

test("the major job moves a tag only: no checkout, no push, no branch", () => {
  assert.doesNotMatch(major, /actions\/checkout|git push|refs\/heads/);
  const r = run("0.22.1", { "v0.22.1": NEW, v0: OLD });
  assert.equal(r.status, 0, r.stderr);
  assert.ok(
    r.writes.every((c) => c.some((a) => /git\/refs\/tags\/v0$|ref=refs\/tags\/v0$/.test(a))),
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
