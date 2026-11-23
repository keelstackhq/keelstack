import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, copyFile, mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { parse } from "smol-toml";
import YAML from "yaml";

const root = (t) => fileURLToPath(new URL(`../${t}/pieces/base/`, import.meta.url));

test("template-rust: scripts/release-pre.sh is a seed no-op bash hook run before hoist release publish", async () => {
  const base = root("template-rust");
  const toml = parse(await readFile(`${base}piece.toml`, "utf8"));
  assert.ok(toml.files.seed.includes("scripts/release-pre.sh"));
  for (const cls of ["owned", "shared"])
    assert.ok(!(toml.files[cls] ?? []).includes("scripts/release-pre.sh"));
  const file = `${base}files/scripts/release-pre.sh`;
  const text = await readFile(file, "utf8");
  assert.match(text, /^#!\/usr\/bin\/env bash\n/);
  assert.match(text, /^set -euo pipefail$/m);
  assert.match(text, /corepack/);
  const r = spawnSync("bash", [file], { encoding: "utf8" });
  assert.equal(r.status, 0, r.stderr);
  const wf = YAML.parse(await readFile(`${base}files/.github/workflows/release.yml`, "utf8"));
  const steps = wf.jobs.publish.steps;
  const hook = steps.findIndex((s) => /bash scripts\/release-pre\.sh/.test(s.run ?? ""));
  const pub = steps.findIndex((s) => /hoist release publish --assets dist/.test(s.run ?? ""));
  assert.ok(hook >= 0 && pub === hook + 1, "hook runs right before hoist release publish");
  assert.match(steps[hook].run, /\[ -f scripts\/release-pre\.sh \]/);
  assert.match(
    await readFile(`${base}files/docs/agents/project.md`, "utf8"),
    /release-pre\.sh[^]*publish rights/,
  );
  assert.match(text, /publish rights/);
  assert.match(await readFile(`${base}files/README.md`, "utf8"), /release-pre\.sh/);
});

test("template-rust: detect asks hoist (dry run) instead of reading crates", async () => {
  const raw = await readFile(`${root("template-rust")}files/.github/workflows/release.yml`, "utf8");
  const wf = YAML.parse(raw);
  const steps = wf.jobs.detect.steps;
  assert.ok(steps.some((s) => /pins\.mjs --github-output/.test(s.run ?? "")));
  assert.ok(steps.some((s) => /hoist@\$HOIST_VERSION/.test(s.run ?? "")));
  const detect = steps.find((s) => s.id === "detect");
  assert.match(detect.run, /hoist release publish --dry-run --json/);
  assert.match(detect.run, /0 \| 2\) ;;/);
  assert.doesNotMatch(detect.run, /-gt 2/);
  // Only an unreleased version (an item hoist would create or update) is pending: a human step
  // or an unreadable registry alone starts no release.
  assert.match(detect.run, /\.action == "create" or \.action == "update"/);
  assert.doesNotMatch(detect.run, /\.action != "ok"/);
  // Before any build: no --assets.
  assert.doesNotMatch(detect.run, /--assets/);
  // The gate runs first; with releasing off nothing else in detect runs.
  assert.equal(steps[0].id, "gate");
  for (const s of steps.slice(1)) assert.equal(s.if, "steps.gate.outputs.enabled == 'true'");
  assert.equal(detect.env.GH_TOKEN, "${{ github.token }}");
  assert.equal(detect.env.HOIST_VERSION, "${{ steps.pins.outputs.hoist }}");
});

// Runs the detect step's script in a one-crate git repo with a stub `hoist` on PATH.
async function runDetect(hoistVersion, stub) {
  const base = root("template-rust");
  const wf = YAML.parse(await readFile(`${base}files/.github/workflows/release.yml`, "utf8"));
  const script = wf.jobs.detect.steps.find((s) => s.id === "detect").run;
  const d = await mkdtemp(join(tmpdir(), "hull-detect-"));
  await mkdir(join(d, ".github/scripts"), { recursive: true });
  await mkdir(join(d, "bin"));
  await mkdir(join(d, "crates/app/src"), { recursive: true });
  await copyFile(
    `${base}files/.github/scripts/release-pending.mjs`,
    join(d, ".github/scripts/release-pending.mjs"),
  );
  await writeFile(join(d, "Cargo.toml"), '[workspace]\nmembers = ["crates/*"]\n');
  await writeFile(join(d, "crates/app/Cargo.toml"), '[package]\nname = "app"\nversion = "0.1.0"\n');
  await writeFile(
    join(d, "bin/hoist"),
    `#!/usr/bin/env bash\necho called >> "${d}/hoist.log"\n${stub}\n`,
  );
  await chmod(join(d, "bin/hoist"), 0o755);
  const git = (...a) => spawnSync("git", a, { cwd: d, encoding: "utf8" });
  git("init", "-q");
  git("add", "-A");
  git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "x");
  const out = join(d, "out.txt");
  await writeFile(out, "");
  const r = spawnSync("bash", ["-e", "-c", script], {
    cwd: d,
    encoding: "utf8",
    env: {
      ...process.env,
      PATH: `${join(d, "bin")}:${process.env.PATH}`,
      HOIST_VERSION: hoistVersion,
      GITHUB_OUTPUT: out,
    },
  });
  const called = await readFile(join(d, "hoist.log"), "utf8").catch(() => "");
  return {
    status: r.status,
    stderr: r.stderr,
    output: await readFile(out, "utf8"),
    called: called !== "",
  };
}

const OLD_HOIST = "exit 3"; // hoist < 0.22.0: the dry run without --assets is a config error
const json = (action) => `echo '{"items":[{"action":"${action}"}]}'`;

test("template-rust detect: the pinned hoist gets the path it supports", async () => {
  const pin = parse(await readFile(`${root("template-rust")}files/pins.toml`, "utf8")).pins.hoist;
  const [maj, min] = pin.split(".").map(Number);
  const supportsDryRun = maj > 0 || min >= 22;
  // A stub that fails like the pinned hoist would on the path it cannot serve.
  const r = await runDetect(pin, supportsDryRun ? `${json("create")}; exit 0` : OLD_HOIST);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.output, /^pending=true$/m);
  assert.equal(r.called, supportsDryRun, `hoist ${pin}: dry run only from 0.22.0`);
});

test("template-rust detect: hoist < 0.22.0 falls back to the crate tags, never runs the dry run", async () => {
  for (const v of ["0.5.1", "0.21.9", "0.9.0"]) {
    const r = await runDetect(v, OLD_HOIST);
    assert.equal(r.status, 0, `${v}: ${r.stderr}`);
    assert.equal(r.called, false, v);
    assert.match(r.output, /^pending=true$/m);
    assert.match(
      r.output,
      /^packages=\[\{"name":"app","version":"0\.1\.0","tag":"app@0\.1\.0"\}\]$/m,
    );
  }
});

test("template-rust detect: hoist >= 0.22.0 plans by dry run; exit 0 and 2 are plans, 1 and 3 fail", async () => {
  for (const v of ["0.22.0", "0.22.1", "0.100.0", "1.0.0"]) {
    const r = await runDetect(v, `${json("create")}; exit 0`);
    assert.equal(r.status, 0, `${v}: ${r.stderr}`);
    assert.ok(r.called, v);
    assert.match(r.output, /^pending=true$/m);
  }
  const human = await runDetect("0.22.0", `${json("pending")}; exit 2`);
  assert.equal(human.status, 0, human.stderr);
  assert.match(human.output, /^pending=false$/m);
  for (const rc of [1, 3]) {
    const r = await runDetect("0.22.0", `${json("create")}; exit ${rc}`);
    assert.equal(r.status, rc);
    assert.match(r.stderr, new RegExp(`dry-run failed \\(${rc}\\)`));
    assert.doesNotMatch(r.output, /pending=/);
  }
});

test("template-rust AGENTS.md: releases run from release.yml, not human tag pushes", async () => {
  const text = await readFile(`${root("template-rust")}files/AGENTS.md.jinja`, "utf8");
  assert.doesNotMatch(text, /tag pushes by a human/);
  assert.match(text, /release\.yml/);
});

test("template release.yml: no explicit --provenance (private repos refuse it)", async () => {
  const raw = await readFile(`${root("template")}files/.github/workflows/release.yml`, "utf8");
  assert.doesNotMatch(raw, /npm publish[^\n]*--provenance/);
  assert.match(raw, /provenance/);
});
