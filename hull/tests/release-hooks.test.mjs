import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
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
  assert.match(await readFile(`${base}files/docs/agents/project.md`, "utf8"), /release-pre\.sh/);
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
  assert.match(detect.run, /-gt 2/);
  assert.match(detect.run, /\.action != "ok"/);
  assert.equal(detect.env.GH_TOKEN, "${{ github.token }}");
  assert.doesNotMatch(raw, /release-pending\.mjs detect/);
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
