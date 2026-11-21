// The release tag of splice is the monorepo's lockstep `vX.Y.Z` (D-023), created by job `release`
// of the root .github/workflows/release.yml. These tests run its step scripts under bash against
// real git repositories and a fake `gh` (./release-yml.mjs), and check the job's structure.
import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fakeGh, gitEnv, job, releaseYml, runStep, stepScript } from "./release-yml.mjs";

const yml = releaseYml();
const release = job(yml, "release");
const TAG_STEP = stepScript(release, "Release commit, tag and GitHub release");
const NOTES_STEP = stepScript(release, "SHA256SUMS and release notes");

/**
 * A bare origin with main at the merge commit SHA, and a clone at SHA with the rebuilt hoard
 * action (as job `release` has it after its checks), the release assets and the notes.
 */
function sandbox({ version = "0.22.0", previous = "", gh = {} } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "release-tag-"));
  const env = gitEnv(dir);
  const g = (cwd, ...args) => execFileSync("git", args, { cwd, env, encoding: "utf8" }).trim();
  const origin = join(dir, "origin.git");
  const work = join(dir, "work");
  g(dir, "init", "-q", "--bare", origin);
  // The step reads and pushes git@github.com:$GH_REPO.git (SSH, the tag deploy key): here that
  // URL is the bare origin.
  appendFileSync(env.GIT_CONFIG_GLOBAL, `[url "${origin}"]\n\tinsteadOf = git@github.com:o/r.git\n`);
  g(dir, "init", "-q", "-b", "main", work);
  g(work, "remote", "add", "origin", origin);
  mkdirSync(join(work, "hoard/action/src"), { recursive: true });
  mkdirSync(join(work, "hoard/action/dist"), { recursive: true });
  writeFileSync(join(work, "hoard/action/src/checksums.generated.ts"), "export const SHAS = {};\n");
  writeFileSync(join(work, "hoard/action/dist/index.js"), "old\n");
  g(work, "add", "-A");
  g(work, "commit", "-q", "-m", "merge");
  const sha = g(work, "rev-parse", "HEAD");
  g(work, "push", "-q", "origin", "main");
  writeFileSync(join(work, "hoard/action/src/checksums.generated.ts"), "export const SHAS = { x: 1 };\n");
  writeFileSync(join(work, "hoard/action/dist/index.js"), "new\n");
  mkdirSync(join(work, "release"));
  writeFileSync(join(work, "release/splice-x86_64-unknown-linux-musl.tar.gz"), "asset\n");
  writeFileSync(join(work, "notes.md"), "notes\n");
  const fake = fakeGh(gh);
  const tag = `v${version}`;
  const run = () =>
    runStep(TAG_STEP, {
      cwd: work,
      env: {
        ...env,
        ...fake.env,
        VERSION: version,
        TAG: tag,
        SHA: sha,
        PREVIOUS: previous,
        GH_TOKEN: "dummy",
        GH_REPO: "o/r",
      },
    });
  return { dir, env, g, origin, work, sha, tag, fake, run };
}

const creates = (calls) => calls.filter((c) => c[0] === "release" && c[1] === "create");

test("an untagged version is tagged and released", () => {
  const s = sandbox({ previous: "v0.21.5" });
  const r = s.run();
  assert.equal(r.status, 0, r.stderr + r.stdout);
  // The tag is the release commit on top of SHA, annotated, and the only ref pushed.
  const commit = s.g(s.origin, "rev-parse", `${s.tag}^{commit}`);
  assert.equal(s.g(s.origin, "rev-parse", `${commit}^`), s.sha);
  assert.equal(s.g(s.origin, "log", "-1", "--format=%s", commit), `chore(release): ${s.tag}`);
  assert.equal(s.g(s.origin, "cat-file", "-t", `refs/tags/${s.tag}`), "tag");
  assert.equal(s.g(s.origin, "rev-parse", "refs/heads/main"), s.sha, "main is never pushed");
  assert.match(s.g(s.origin, "show", `${commit}:hoard/action/dist/index.js`), /^new$/);
  assert.match(r.output, new RegExp(`^commit=${commit}$`, "m"));
  const [create] = creates(s.fake.calls());
  assert.ok(create, "gh release create was not called");
  assert.deepEqual(create.slice(2, 8), [s.tag, "--draft", "--target", s.sha, "--title", s.tag]);
  assert.ok(create.includes("--generate-notes"));
  // The draft is published once the tag is pushed.
  assert.deepEqual(
    s.fake.state().releases.map((x) => [x.tag, x.draft]),
    [[s.tag, false]],
  );
});

test("the notes start at the previous release only when there is one", () => {
  const withPrev = sandbox({ previous: "v0.21.5" });
  assert.equal(withPrev.run().status, 0);
  const [a] = creates(withPrev.fake.calls());
  assert.equal(a[a.indexOf("--notes-start-tag") + 1], "v0.21.5");
  const first = sandbox();
  assert.equal(first.run().status, 0);
  const [b] = creates(first.fake.calls());
  assert.ok(!b.includes("--notes-start-tag"), "the first release: GitHub's default notes");
});

test("a tag on another commit releases nothing", () => {
  for (const annotated of [false, true]) {
    const s = sandbox();
    s.g(s.work, "commit", "-q", "--allow-empty", "-m", "other");
    const other = s.g(s.work, "rev-parse", "HEAD");
    // Annotated: the peeled commit counts, not the tag object.
    if (annotated) s.g(s.work, "tag", "-a", s.tag, "-m", s.tag, other);
    else s.g(s.work, "tag", s.tag, other);
    s.g(s.work, "reset", "-q", "--soft", s.sha);
    s.g(s.work, "push", "-q", "origin", `refs/tags/${s.tag}`);
    const r = s.run();
    assert.equal(r.status, 1, `annotated ${annotated}`);
    assert.match(r.stdout, /not the release commit of .*nothing was released by this run/);
    assert.equal(s.g(s.origin, "rev-parse", `${s.tag}^{commit}`), other, "the tag is untouched");
    assert.deepEqual(creates(s.fake.calls()), []);
  }
});

test("a re-run after a finished release does nothing", () => {
  const s = sandbox();
  assert.equal(s.run().status, 0);
  const commit = s.g(s.origin, "rev-parse", `${s.tag}^{commit}`);
  const again = s.run();
  assert.equal(again.status, 0, again.stderr + again.stdout);
  assert.match(again.stdout, /is released already: nothing to do/);
  assert.match(again.output, new RegExp(`^commit=${commit}$`, "m"));
  assert.equal(creates(s.fake.calls()).length, 1, "no second release");
});

test("a tag without a release, or a release without its tag, stops for a person", () => {
  const s = sandbox();
  assert.equal(s.run().status, 0);
  // The release is gone but the tag stays: the baked checksums belong to lost assets.
  const noRelease = { ...s, fake: fakeGh() };
  const r = runStep(TAG_STEP, {
    cwd: s.work,
    env: { ...s.env, ...noRelease.fake.env, VERSION: "0.22.0", TAG: s.tag, SHA: s.sha, PREVIOUS: "", GH_TOKEN: "x", GH_REPO: "o/r" },
  });
  assert.equal(r.status, 1);
  assert.match(r.stdout, /exists without a release.*Delete the tag/);
  // A published release whose tag is missing is never recreated.
  const t = sandbox({ gh: { releases: [{ id: 7, tag: "v0.22.0", draft: false }], next: 8 } });
  const p = t.run();
  assert.equal(p.status, 1);
  assert.match(p.stdout, /is published but its tag is missing/);
  assert.throws(() => t.g(t.origin, "rev-parse", "--verify", "-q", `refs/tags/${t.tag}`));
});

test("a draft left without a tag by an earlier run is replaced", () => {
  const s = sandbox({ gh: { releases: [{ id: 3, tag: "v0.22.0", draft: true }], next: 4 } });
  const r = s.run();
  assert.equal(r.status, 0, r.stderr + r.stdout);
  assert.ok(s.fake.calls().some((c) => c.includes("DELETE") && c.includes("repos/o/r/releases/3")));
  assert.deepEqual(
    s.fake.state().releases.map((x) => [x.id, x.draft]),
    [[4, false]],
  );
});

test("other tags do not count, also not as prefixes", () => {
  const s = sandbox();
  s.g(s.work, "commit", "-q", "--allow-empty", "-m", "other");
  for (const t of ["v0.22.10", "v0.22.0-rc.1", "v0", "v0.2"]) s.g(s.work, "tag", t);
  s.g(s.work, "reset", "-q", "--soft", s.sha);
  s.g(s.work, "push", "-q", "origin", "--tags");
  const r = s.run();
  assert.equal(r.status, 0, r.stderr + r.stdout);
  assert.equal(s.g(s.origin, "rev-parse", `${s.tag}^{commit}^`), s.sha);
});

test("the notes start at the previous vX.Y.Z release, never at v<major>", () => {
  const dir = mkdtempSync(join(tmpdir(), "release-notes-"));
  const env = gitEnv(dir);
  const g = (...args) => execFileSync("git", args, { cwd: dir, env, encoding: "utf8" }).trim();
  g("init", "-q");
  g("commit", "-q", "--allow-empty", "-m", "one");
  for (const t of ["v0", "v1", "v0.1.0", "v0.2.0", "v0.2.1", "v0.3.0-rc.1", "v0.10.0", "vx"]) g("tag", t);
  const previous = (tag) => {
    mkdirSync(join(dir, "release"), { recursive: true });
    writeFileSync(join(dir, "release/a.tar.gz"), "a\n");
    const r = runStep(NOTES_STEP, { cwd: dir, env: { ...env, VERSION: tag.slice(1), TAG: tag } });
    assert.equal(r.status, 0, r.stderr);
    return r.output.match(/^PREVIOUS=(.*)$/m)[1];
  };
  assert.equal(previous("v0.2.1"), "v0.2.0");
  assert.equal(previous("v0.10.0"), "v0.2.1"); // numeric, not text order; rc skipped
  assert.equal(previous("v0.3.0"), "v0.2.1");
  assert.equal(previous("v0.11.0"), "v0.10.0"); // a tag not yet created still finds its place
  assert.equal(previous("v0.1.0"), ""); // the first release: GitHub's default
});

test("release.yml tags only on the dispatched run on main, behind the release environment", () => {
  assert.match(release, /needs: \[plan, build, action\]/);
  assert.match(release, /github\.event_name == 'workflow_dispatch' && needs\.plan\.outputs\.release == 'true'/);
  assert.match(release, /needs\.plan\.outputs\.released != 'true'/);
  assert.match(release, /\n    environment: release\n/);
  assert.match(release, /\n    permissions:\n      contents: write\n    outputs:/);
  // The tag comes from the plan (Cargo.toml's version), never from the event.
  assert.match(release, /TAG: \$\{\{ needs\.plan\.outputs\.tag \}\}/);
  assert.doesNotMatch(release, /github\.ref_name|github\.event\.|inputs\./);
  // Only the tag is pushed, never forced, over SSH with the tag deploy key (the tags ruleset's
  // only bypass actor), before the release is published: the API never creates the tag.
  assert.match(TAG_STEP, /remote="git@github\.com:\$GH_REPO\.git"\n/);
  assert.match(TAG_STEP, /git push "\$remote" "refs\/tags\/\$TAG"\n/);
  assert.doesNotMatch(TAG_STEP, /--force|\+refs|push (origin|"\$remote") (main|HEAD)/);
  assert.doesNotMatch(TAG_STEP, /extraheader|GH_TOKEN/, "the token never reaches git");
  assert.ok(
    TAG_STEP.indexOf('git push "$remote"') < TAG_STEP.lastIndexOf('publish "$rid"'),
    "the tag is pushed before the release is published",
  );
  assert.match(release, /RELEASE_TAG_KEY: \$\{\{ secrets\.RELEASE_TAG_KEY \}\}/);
  assert.match(TAG_STEP, /--notes-start-tag "\$PREVIOUS"/);
});

test("release.yml builds the commit of the run and never cancels a release", () => {
  for (const name of ["release", "build", "action"]) {
    assert.match(job(yml, name), /ref: \$\{\{ needs\.plan\.outputs\.sha \}\}/, name);
  }
  assert.doesNotMatch(yml, /cancel-in-progress: true/);
  assert.match(yml, /cancel-in-progress: false/);
  // The plan tags vX.Y.Z of Cargo.toml's version at the merge commit.
  assert.match(job(yml, "plan"), /echo "tag=v\$version"/);
});
