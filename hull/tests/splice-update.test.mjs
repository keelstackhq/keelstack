import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { cp, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { git } from "../scripts/lib/splice.mjs";

const BIN = process.env.SPLICE_BIN;
const HULL = fileURLToPath(new URL("..", import.meta.url));
const TEMPLATE = join(HULL, "template");
const WEB = ["preact-app:web", "ui-storybook", "hono-worker:api"];

const SEMVER = /^v\d+\.\d+\.\d+$/;
const bySemver = (a, b) => {
  const [x, y] = [a, b].map((t) => t.slice(1).split(".").map(Number));
  return x[0] - y[0] || x[1] - y[1] || x[2] - y[2];
};

/**
 * hull's release tags from keelstackhq/hull (not imported, D-023), each mapped to its commit in this
 * repository (`legacy-tags.txt`). Tags of this repository's releases (> 0.21.0) come after them.
 */
const LEGACY = new Map(
  readFileSync(join(HULL, "legacy-tags.txt"), "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.split(" ")),
);

/** Commit of `v<semver>` tag `tag`: a tag of this repository, else a legacy hull tag. */
function tagCommit(tag) {
  const r = spawnSync("git", ["rev-parse", "--verify", "--quiet", `refs/tags/${tag}^{commit}`], {
    cwd: HULL,
    encoding: "utf8",
  });
  return r.status === 0 ? r.stdout.trim() : LEGACY.get(tag);
}

/** Latest `v<semver>` tag reachable from HEAD (this repository's or a legacy hull tag), or undefined. */
function previousTag() {
  const r = spawnSync("git", ["tag", "--merged", "HEAD", "--list", "v*"], {
    cwd: HULL,
    encoding: "utf8",
  });
  const own = r.status === 0 ? r.stdout.split("\n").filter((t) => SEMVER.test(t)) : [];
  const legacy = [...LEGACY].filter(([, sha]) => {
    const a = spawnSync("git", ["merge-base", "--is-ancestor", sha, "HEAD"], { cwd: HULL });
    return a.status === 0;
  });
  return [...own, ...legacy.map(([t]) => t)].sort(bySemver).at(-1);
}

const prev = previousTag();
const skip = !BIN
  ? "SPLICE_BIN is unset (hull CI sets it; see scripts/lib/splice.mjs)"
  : !prev && !process.env.CI && "no v<semver> tag reachable from HEAD (fetch tags)";

/**
 * A template repo with tag `from` (that tag's template/) and `v<next>` (the working tree).
 * @param {string} root
 * @param {string} [from] a `v<semver>` tag of this repo; defaults to the latest one
 */
async function templateRepo(root, from = prev) {
  assert.ok(
    from,
    "no v<semver> tag reachable from HEAD; CI must fetch the history (fetch-depth: 0)",
  );
  const repo = join(root, "tpl");
  await mkdir(repo, { recursive: true });
  git(repo, "init", "-q");
  const commit = tagCommit(from);
  assert.ok(commit, `no commit for ${from}: not a tag here and not in legacy-tags.txt`);
  // Run in hull/: archive paths are relative to it (template/), as in hull's own repository.
  const tar = spawnSync("git", ["archive", commit, "template"], { cwd: HULL });
  assert.equal(tar.status, 0, String(tar.stderr));
  const x = spawnSync("tar", ["-x", "-C", repo], { input: tar.stdout });
  assert.equal(x.status, 0, String(x.stderr));
  // v0.1.0 shipped migrations/.gitkeep, which splice 0.2.0 refuses as a migration file.
  // Drop this line once the previous tag is newer than v0.1.0.
  await rm(join(repo, "template/migrations/.gitkeep"), { force: true });
  git(repo, "add", "-A");
  git(repo, "commit", "-q", "-m", from);
  git(repo, "tag", from);
  await rm(join(repo, "template"), { recursive: true });
  await cp(TEMPLATE, join(repo, "template"), { recursive: true });
  git(repo, "add", "-A");
  git(repo, "commit", "-q", "--allow-empty", "-m", "next");
  const [maj, min] = from.slice(1).split(".").map(Number);
  const next = `v${maj}.${min + 1}.0`;
  git(repo, "tag", next);
  return { source: `${repo}//template`, next };
}

const run = (cwd, ...args) => spawnSync(BIN, args, { cwd, encoding: "utf8" });

test("update keeps seed, merges shared, guards owned", { skip }, async (t) => {
  const root = await mkdtemp(join(tmpdir(), "hull-update-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const { source, next } = await templateRepo(root);
  const app = join(root, "app");
  const pieces = WEB.flatMap((p) => ["--piece", p]);
  const opts = ["--data", "scope=hullci", "--yes", "--json", "--no-post"];
  const made = run(root, "new", source, app, "--ref", prev, ...pieces, ...opts);
  assert.equal(made.status, 0, made.stdout + made.stderr);
  git(app, "init", "-q");
  git(app, "add", "-A");
  git(app, "commit", "-q", "-m", "new");

  const seed = join(app, "apps/web/src/app.tsx");
  const editedSeed = `${await readFile(seed, "utf8")}// my edit\n`;
  await writeFile(seed, editedSeed);
  const pkgPath = join(app, "apps/web/package.json");
  const pkg = JSON.parse(await readFile(pkgPath, "utf8"));
  pkg.dependencies = { ...pkg.dependencies, "left-pad": "1.3.0" };
  await writeFile(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);
  const owned = join(app, "apps/web/vite.config.ts");
  await writeFile(owned, `${await readFile(owned, "utf8")}// local tweak\n`);
  git(app, "add", "-A");
  git(app, "commit", "-q", "-m", "user edits");

  const check = run(app, "check", "--json", "--offline");
  assert.equal(check.status, 1, check.stdout + check.stderr);
  assert.match(check.stdout, /apps\/web\/vite\.config\.ts/);

  const refused = run(app, "update", "--to", next, "--yes", "--json", "--no-post");
  assert.equal(refused.status, 1, refused.stdout + refused.stderr);
  assert.match(await readFile(owned, "utf8"), /local tweak/);

  const up = run(app, "update", "--to", next, "--force", "--yes", "--json", "--no-post");
  assert.equal(up.status, 0, up.stdout + up.stderr);
  assert.equal(JSON.parse(up.stdout).ok, true);

  assert.equal(await readFile(seed, "utf8"), editedSeed);
  assert.equal(JSON.parse(await readFile(pkgPath, "utf8")).dependencies["left-pad"], "1.3.0");
  assert.equal(
    await readFile(owned, "utf8"),
    await readFile(join(TEMPLATE, "pieces/preact-app/files/vite.config.ts"), "utf8"),
  );
  // base ships the four subagents again (owned): the update creates them from the template
  for (const f of ["explore", "implementer", "reviewer", "ci-fixer"]) {
    assert.equal(
      await readFile(join(app, `.claude/agents/${f}.md`), "utf8"),
      await readFile(join(TEMPLATE, `pieces/base/files/.claude/agents/${f}.md`), "utf8"),
      f,
    );
  }
  const final = run(app, "check", "--json", "--offline");
  assert.equal(final.status, 0, final.stdout + final.stderr);
});

// Repos first rendered before v0.1.0 (dev renderer) and later adopted with `splice new --force`
// still carry `.claude/agents/explorer.md`, which no lock tracks (migrations/0.2.1.toml). The
// update starts at v0.2.0 so that migration is in range whatever the latest tag is.
const FROM_EXPLORER = "v0.2.0";
test(
  "update deletes an unedited pre-0.1.0 explorer.md, keeps an edited one",
  { skip },
  async (t) => {
    const root = await mkdtemp(join(tmpdir(), "hull-update-"));
    t.after(() => rm(root, { recursive: true, force: true }));
    const { source, next } = await templateRepo(root, FROM_EXPLORER);
    // The copy hull shipped (git blob ad17ec0f; tests/ai-layer.test.mjs checks the hash).
    const shipped = await readFile(join(HULL, "tests/fixtures/explorer/explorer.md"), "utf8");
    for (const [name, content, kept] of [
      ["unedited", shipped, false],
      ["edited", `${shipped}my note\n`, true],
    ]) {
      const app = join(root, name);
      const opts = ["--data", "scope=hullci", "--yes", "--json", "--no-post"];
      const made = run(root, "new", source, app, "--ref", FROM_EXPLORER, ...opts);
      assert.equal(made.status, 0, made.stdout + made.stderr);
      const explorer = join(app, ".claude/agents/explorer.md");
      await mkdir(join(app, ".claude/agents"), { recursive: true });
      await writeFile(explorer, content);
      git(app, "add", "-A");
      git(app, "commit", "-q", "-m", "old render");
      const up = run(app, "update", "--to", next, "--yes", "--json", "--no-post");
      assert.equal(up.status, 0, up.stdout + up.stderr);
      assert.equal(existsSync(explorer), kept, `${name}: ${up.stdout}`);
    }
  },
);
