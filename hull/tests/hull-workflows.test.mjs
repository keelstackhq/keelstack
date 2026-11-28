import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, readdir, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import YAML from "yaml";

// hull/.github/workflows/ is kept from the old keelstackhq/hull repo but does not run here; hull's
// CI is the `ci-hull` job of the monorepo's root ci.yml and its release is the root release.yml.
const DIR = fileURLToPath(new URL("../.github/workflows/", import.meta.url));
const ROOT_DIR = fileURLToPath(new URL("../../.github/workflows/", import.meta.url));

test("hull's own and the root workflows are valid YAML with jobs", async () => {
  for (const dir of [DIR, ROOT_DIR]) {
    const files = (await readdir(dir)).filter((f) => /\.ya?ml$/.test(f));
    assert.ok(files.length > 0, dir);
    for (const f of files) {
      const doc = YAML.parseDocument(await readFile(dir + f, "utf8"));
      assert.deepEqual(doc.errors.map(String), [], f);
      assert.ok(doc.toJS().jobs, `${f} has jobs`);
    }
  }
});

const hullCi = async () =>
  YAML.parse(await readFile(ROOT_DIR + "ci.yml", "utf8")).jobs["ci-hull"].steps;
const combosJson = async () => {
  const { loadCombos } = await import("../scripts/lib/combos.mjs");
  return JSON.stringify(loadCombos());
};

test("root ci.yml ci-hull Rust condition matches rust-tool and rust-cli combos only", async () => {
  const step = (await hullCi()).find((s) => s.id === "rust");
  const script = step.run.match(/node -e '([^']+)'/)[1];
  const exit = async (list) =>
    spawnSync("node", ["-e", script, JSON.stringify(list), await combosJson()]).status;
  assert.equal(await exit(["cli"]), 0);
  assert.equal(await exit(["site"]), 0);
  assert.equal(await exit(["web"]), 1);
  assert.equal(await exit(["lib"]), 1);
  assert.equal(await exit(["rust"]), 0);
});

test("root ci.yml ci-hull installs nextest and cargo-deny at the .bosun/pins.toml versions, with rust-cache for the cli and rust combos", async () => {
  const steps = await hullCi();
  const install = steps.find((s) => s.uses?.startsWith("taiki-e/install-action"));
  assert.equal(install.with.tool, "${{ steps.pins.outputs.tools }}");
  assert.equal(install.if, "steps.rust.outputs.needed == 'true'");
  // One source: the pins step reads .bosun/pins.toml; run it and compare with the file.
  const pins = steps.find((s) => s.id === "pins");
  assert.ok(pins && steps.indexOf(pins) < steps.indexOf(install), "pins step before install");
  const root = fileURLToPath(new URL("../..", import.meta.url));
  // Run it where Actions would: the step's working-directory, else the job's `defaults.run`.
  const job = YAML.parse(await readFile(ROOT_DIR + "ci.yml", "utf8")).jobs["ci-hull"];
  const wd = (pins["working-directory"] ?? job.defaults?.run?.["working-directory"] ?? ".").replace(
    "${{ github.workspace }}",
    root,
  );
  const out = join(await mkdtemp(join(tmpdir(), "pins-")), "out");
  const r = spawnSync("bash", ["-euo", "pipefail", "-c", pins.run], {
    cwd: wd.startsWith("/") ? wd : join(root, wd),
    env: { ...process.env, GITHUB_OUTPUT: out },
    encoding: "utf8",
  });
  assert.equal(r.status, 0, r.stderr);
  const toml = await readFile(join(root, ".bosun/pins.toml"), "utf8");
  const ver = (t) => toml.match(new RegExp(`^\\[${t}\\]\\nversion = "([^"]+)"`, "m"))[1];
  assert.equal(
    (await readFile(out, "utf8")).trim(),
    `tools=cargo-nextest@${ver("nextest")},cargo-deny@${ver("cargo-deny")}`,
  );
  // One cache for the workspace's splice build (always) and the two Rust combos.
  const cache = steps.find((s) => s.uses?.startsWith("Swatinem/rust-cache@"));
  assert.equal(
    cache.with.workspaces,
    ". -> target\n${{ runner.temp }}/combos/cli -> target\n${{ runner.temp }}/combos/rust -> target\n",
  );
});

// In the monorepo hull tests the workspace's splice (hull and splice change the contract together).
test("root ci.yml ci-hull builds the workspace splice before Test and sets SPLICE_BIN", async () => {
  const steps = await hullCi();
  const i = steps.findIndex((s) => s.name === "Build splice (workspace)");
  assert.ok(i >= 0, "Build splice (workspace) step");
  assert.ok(i < steps.findIndex((s) => s.name === "Test"), "before Test");
  assert.match(steps[i].run, /cargo build [^\n]*--locked -p keel-splice --bin splice/);
  assert.match(steps[i].run, /SPLICE_BIN=.*>> "\$GITHUB_ENV"/);
});

// The old hull repo's own splice update bot (hull/.github/workflows/splice.yml) does not run in
// the monorepo; the bot lives on in the splice.yml every rendered repo gets from the templates.
test("the templates' splice.yml runs the splice update bot on all sources", async () => {
  for (const t of ["template", "template-rust"]) {
    const file = fileURLToPath(
      new URL(`../${t}/pieces/base/files/.github/workflows/splice.yml`, import.meta.url),
    );
    const wf = YAML.parse(await readFile(file, "utf8"));
    assert.ok(wf.on.schedule?.length, `${t}: scheduled`);
    assert.ok("workflow_dispatch" in wf.on, t);
    assert.deepEqual(wf.permissions, { contents: "write", "pull-requests": "write" }, t);
    const steps = wf.jobs.update.steps;
    const checkout = steps[0];
    const bot = steps.find((s) =>
      String(s.uses).startsWith("keelstackhq/keelstack/splice/action@"),
    );
    assert.match(checkout.uses, /^actions\/checkout@/, t);
    assert.equal(checkout.with["persist-credentials"], false, t);
    // it receives a token with workflows: write, so it is pinned by commit SHA
    assert.equal(
      bot.uses,
      "keelstackhq/keelstack/splice/action@5816246c9d0166474424c9a877c7762ba15b7f4b",
      t,
    );
    assert.equal(bot.with.scope, "all", t);
    assert.equal(
      bot.with.token,
      "${{ steps.app.outputs.token || secrets.SPLICE_BOT_TOKEN || github.token }}",
      t,
    );
  }
});

test("root ci.yml ci-hull uses the in-repo hoard action for the turbo cache before dogfood, without --cache-dir", async () => {
  const steps = await hullCi();
  // A workflow of this repository runs its own hoard (./hoard), never a remote pin of it.
  assert.ok(
    !steps.some((s) => (s.uses ?? "").startsWith("keelstackhq/")),
    "no remote keelstackhq action",
  );
  const i = steps.findIndex((s) => s.uses === "./hoard");
  assert.ok(i >= 0, "uses ./hoard");
  const co = steps.findIndex((s) => s.uses?.startsWith("actions/checkout@"));
  assert.ok(co >= 0 && co < i, "checkout before the local action");
  assert.equal(steps[i].if, "steps.combos.outputs.args != ''");
  const df = steps.findIndex((s) => s.name === "Dogfood combos");
  assert.ok(i < df, "before dogfood");
  assert.doesNotMatch(steps[df].run, /--cache-dir/);
  assert.ok(!steps.some((s) => s.uses?.startsWith("actions/cache")), "no actions/cache step");
});

// hull's per-folder release moved to the monorepo's lockstep release (root release.yml, D-023).
test("root release.yml plans on merged PRs to main and releases behind the release environment", async () => {
  const text = await readFile(ROOT_DIR + "release.yml", "utf8");
  const wf = YAML.parse(text);
  assert.equal(wf.name, "release");
  assert.deepEqual(wf.on.pull_request, {
    types: ["closed"],
    branches: ["main"],
  });
  assert.ok("workflow_dispatch" in wf.on);
  assert.deepEqual(wf.permissions, {});
  assert.equal(wf.concurrency["cancel-in-progress"], false);
  // bosun's ci-minutes gate wants cancelling concurrency; its documented allow comment keeps a
  // release from ever being cancelled mid-publish.
  assert.match(text, /^# ci-minutes: allow concurrency \S/m);
  const { plan, draft, publish, release } = wf.jobs;
  // The repo guard names this repository, never an old per-tool one.
  assert.match(plan.if, /^github\.repository == 'keelstackhq\/keelstack' &&/);
  assert.doesNotMatch(text, /github\.repository == 'keelstackhq\/(?!keelstack')/);
  assert.deepEqual(plan.permissions, { contents: "read" });
  // A PR run never publishes: everything that writes needs a workflow_dispatch run (on main).
  const writers = Object.entries(wf.jobs).filter(([, j]) => j.permissions?.contents === "write");
  assert.deepEqual(
    writers.map(([k]) => k),
    ["draft", "release"],
  );
  // The draft job has no environment and runs no dependency code; the one tap is on `publish`
  // (environment release, contents: read + id-token: write, trusted publishing, no tag key);
  // the tag key lives in release-tags, read only by `release`, which writes the tag.
  assert.equal(draft.environment, undefined);
  assert.equal(publish.environment, "release");
  assert.deepEqual(publish.permissions, { contents: "read", "id-token": "write" });
  assert.equal(release.environment, "release-tags");
  assert.deepEqual(release.permissions, { contents: "write" });
  // Exactly two environments: the owner's tap and the tag key's.
  const envs = new Set(
    Object.values(wf.jobs)
      .map((j) => j.environment)
      .filter(Boolean),
  );
  assert.deepEqual([...envs].sort(), ["release", "release-tags"]);
  const idToken = Object.entries(wf.jobs).filter(([, j]) => j.permissions?.["id-token"]);
  assert.deepEqual(
    idToken.map(([k]) => k),
    ["publish"],
  );
  for (const [name, job] of Object.entries(wf.jobs)) {
    if (job.environment === undefined) continue;
    assert.match(job.if, /github\.event_name == 'workflow_dispatch'/, name);
  }
  assert.ok(
    wf.jobs.major === undefined && wf.jobs.crates === undefined && wf.jobs.npm === undefined,
  );
  assert.ok(publish.needs.includes("draft"));
  assert.ok(release.needs.includes("publish") && release.needs.includes("draft"));
  assert.match(release.if, /needs\.plan\.outputs\.release == 'true'/);
  const runOf = (job) => job.steps.map((s) => s.run ?? "").join("\n");
  // The draft carries the release notes; the tag job pushes the tag and publishes the draft.
  assert.match(runOf(draft), /--notes-start-tag/);
  assert.doesNotMatch(runOf(release), /gh release create/);
  assert.doesNotMatch(
    runOf(release),
    /cargo publish|cargo build|pnpm install|npm install|npm publish/,
  );
  // The tag goes over SSH with the release-tags deploy key, not the job token.
  const run = runOf(release);
  assert.match(run, /remote="git@github\.com:\$GH_REPO\.git"/);
  assert.match(run, /git push "\$remote" "refs\/tags\/\$TAG"/);
  const co = release.steps.find((s) => s.uses?.startsWith("actions/checkout@"));
  assert.equal(co.with.ref, "${{ needs.plan.outputs.sha }}");
  assert.equal(co.with["persist-credentials"], false);
});

// setup-node@v5 turns on the package-manager cache by itself when package.json has
// `packageManager`; without pnpm on PATH it then fails ("Unable to locate executable file: pnpm").
// setup-node v4 (bosun's workflows pin it) has no automatic cache, so its steps need no opt-out.
const SETUP_NODE_V4 = "actions/setup-node@49933ea5288caeca8642d1e84afbd3f7d6820020";
test("every setup-node step disables the package-manager cache unless pnpm is set up first under the same condition", async () => {
  // The live workflows (the monorepo root's) and the template's; not the inert hull/.github copy.
  const TEMPLATE_DIR = fileURLToPath(
    new URL("../template/pieces/base/files/.github/workflows/", import.meta.url),
  );
  const dirs = [ROOT_DIR, TEMPLATE_DIR];
  let steps = 0;
  // setup-node reads `packageManager` from the workspace root's package.json. The monorepo root
  // has none today, so its workflows need the opt-out only once one appears.
  const rootPkg = fileURLToPath(new URL("../../package.json", import.meta.url));
  const rootPm =
    existsSync(rootPkg) && "packageManager" in JSON.parse(await readFile(rootPkg, "utf8"));
  const bad = [];
  for (const dir of dirs)
    for (const f of (await readdir(dir)).filter((n) => n.endsWith(".yml"))) {
      const wf = YAML.parse(await readFile(dir + f, "utf8"));
      for (const [name, job] of Object.entries(wf.jobs ?? {})) {
        const pnpm = new Set(); // `if` conditions under which pnpm is set up ("" = always)
        for (const step of job.steps ?? []) {
          if (String(step.uses).startsWith("pnpm/action-setup")) pnpm.add(step.if ?? "");
          if (!String(step.uses).startsWith("actions/setup-node")) continue;
          if (dir === ROOT_DIR) {
            steps++;
            if (!rootPm) continue;
          }
          if (step.uses === SETUP_NODE_V4 && !step.with?.cache) continue;
          const covered = pnpm.has("") || pnpm.has(step.if ?? "");
          if (!covered && step.with?.["package-manager-cache"] !== false)
            bad.push(`${dir === ROOT_DIR ? "root" : "template"}/${f} ${name}`);
        }
      }
    }
  assert.deepEqual(bad, []);
  assert.ok(steps > 0, "the root workflows have setup-node steps to check");
});
