// system.toml (D-031, hoist spec §17) seeded by both templates, and the weekly system-check.yml.
// The checks mirror `hoist system check` offline (§17.4) on every rendered combo: no secret a
// workflow reads is undeclared, consumers match the jobs that read each key, metadata is there
// except the dates `hoist system stamp` records. The file also validates against hoist's schema.
import assert from "node:assert/strict";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { parse as parseToml } from "smol-toml";
import YAML from "yaml";
import { comboTemplate, loadCombos } from "../scripts/lib/combos.mjs";
import { renderNew } from "../scripts/render.mjs";
import { check as ciMinutes } from "./ci-minutes-gate.mjs";

const HULL = fileURLToPath(new URL("..", import.meta.url));
const SCHEMA = JSON.parse(
  readFileSync(new URL("../../hoist/schema/system.schema.json", import.meta.url), "utf8"),
);
const { vars: VARS, combos: COMBOS } = loadCombos();

async function render(combo) {
  const dir = join(await mkdtemp(join(tmpdir(), "hull-system-")), "out");
  const vars = { ...(combo.vars ?? VARS) };
  if (combo.pieces.includes("secrets")) vars.context_prefix ??= "hullci";
  const result = await renderNew({
    template: join(HULL, comboTemplate(combo)),
    out: dir,
    pieces: combo.pieces,
    vars,
  });
  return { dir, result };
}

/** A minimal JSON Schema (draft 2020-12) check for the keywords system.schema.json uses. */
function validate(schema, value, path = "$", root = schema) {
  const errs = [];
  if (schema.$ref) return validate(resolve(root, schema.$ref), value, path, root);
  const type = Array.isArray(value) ? "array" : value === null ? "null" : typeof value;
  if (schema.type && schema.type !== type) return [`${path}: ${type}, want ${schema.type}`];
  if ("const" in schema && value !== schema.const) errs.push(`${path}: not ${schema.const}`);
  if (schema.enum && !schema.enum.includes(value)) errs.push(`${path}: ${value} not in enum`);
  if (schema.pattern && !new RegExp(schema.pattern, "u").test(value))
    errs.push(`${path}: ${value} !~ ${schema.pattern}`);
  if (schema.minLength && value.length < schema.minLength) errs.push(`${path}: too short`);
  if (schema.anyOf && !schema.anyOf.some((s) => !validate(s, value, path, root).length))
    errs.push(`${path}: matches no anyOf`);
  if (
    schema.oneOf &&
    schema.oneOf.filter((s) => !validate(s, value, path, root).length).length !== 1
  )
    errs.push(`${path}: matches not exactly one oneOf`);
  for (const s of schema.allOf ?? [])
    if (!validate(s.if, value, path, root).length)
      errs.push(...validate(s.then, value, path, root));
  if (type === "object") {
    for (const k of schema.required ?? []) if (!(k in value)) errs.push(`${path}: missing ${k}`);
    for (const [k, v] of Object.entries(value)) {
      const sub = schema.properties?.[k];
      if (sub) errs.push(...validate(sub, v, `${path}.${k}`, root));
      else if (schema.additionalProperties === false) errs.push(`${path}: unknown key ${k}`);
    }
  }
  if (type === "array" && schema.items)
    value.forEach((v, i) => errs.push(...validate(schema.items, v, `${path}[${i}]`, root)));
  return errs;
}
const resolve = (root, ref) =>
  ref
    .replace(/^#\//, "")
    .split("/")
    .reduce((o, k) => o[k], root);

test("the schema validator catches an unknown key, a bad enum and a missing field", () => {
  const ok = { version: 1, system: { name: "a/b", owner: "x" } };
  assert.deepEqual(validate(SCHEMA, ok), []);
  assert.ok(validate(SCHEMA, { ...ok, extra: 1 }).length);
  const bad = { ...ok, credential: [{ id: "a", provider: "p", kind: "pat", role: "r" }] };
  assert.ok(validate(SCHEMA, bad).length);
  const store = { id: "a", provider: "p", kind: "other", role: "r", storage: [{ kind: "sops" }] };
  assert.equal(validate(SCHEMA, { ...ok, credential: [store] }).length, 2); // no key, no path
});

/** What hoist reads from the rendered repo: secret -> jobs (`file:job`) and job environments. */
function repoUses(dir) {
  const jobs = new Map();
  const envs = new Map();
  const wfDir = join(dir, ".github/workflows");
  const scan = (node) => {
    const out = new Set();
    for (const m of JSON.stringify(node ?? {}).matchAll(/\$\{\{(.*?)\}\}/g))
      for (const s of m[1].matchAll(/\bsecrets\.([A-Za-z_][A-Za-z0-9_]*)/g))
        if (s[1] !== "GITHUB_TOKEN") out.add(s[1]);
    return out;
  };
  for (const f of existsSync(wfDir) ? readdirSync(wfDir) : []) {
    if (!/\.ya?ml$/.test(f)) continue;
    const wf = YAML.parse(readFileSync(join(wfDir, f), "utf8"));
    const { jobs: all = {}, ...outer } = wf;
    const top = scan(outer);
    for (const [id, job] of Object.entries(all)) {
      const ref = `${f}:${id}`;
      envs.set(ref, typeof job.environment === "object" ? job.environment?.name : job.environment);
      for (const s of new Set([...top, ...scan(job)])) {
        if (!jobs.has(s)) jobs.set(s, new Set());
        jobs.get(s).add(ref);
      }
    }
  }
  const listed = new Set();
  if (existsSync(join(dir, "secrets.toml"))) {
    const m = parseToml(readFileSync(join(dir, "secrets.toml"), "utf8"));
    for (const spec of Object.values(m.env ?? {})) for (const k of Object.keys(spec)) listed.add(k);
  }
  return { jobs, envs, listed };
}

const piecesOf = (combo) => combo.pieces.map((p) => p.split(":")[0]);

for (const combo of COMBOS) {
  test(`system.toml (${combo.name}): seed, schema-valid, matches the workflows (hoist §17.4)`, async () => {
    const { dir, result } = await render(combo);
    const cls = Object.fromEntries(result.changes.map((c) => [c.path, c.class]));
    assert.equal(cls["system.toml"], "seed");
    assert.equal(cls[".github/workflows/system-check.yml"], "owned");
    assert.equal(cls[".github/scripts/system-issue.mjs"], undefined, "no repo script edits issues");
    const text = readFileSync(join(dir, "system.toml"), "utf8");
    assert.ok(!/\n\n\n/.test(text), "no triple newline");
    assert.ok(!/\{\{|\{%/.test(text), "fully rendered");
    const sys = parseToml(text);
    assert.deepEqual(validate(SCHEMA, sys), []);
    assert.equal(sys.version, 1);

    const providers = new Set((sys.provider ?? []).map((p) => p.id));
    assert.equal(providers.size, (sys.provider ?? []).length, "unique provider ids");
    const creds = sys.credential ?? [];
    assert.equal(new Set(creds.map((c) => c.id)).size, creds.length, "unique credential ids");
    const { jobs, envs, listed } = repoUses(dir);
    const declared = new Set(creds.flatMap((c) => (c.storage ?? []).map((s) => s.key)));
    for (const name of [...jobs.keys(), ...listed])
      assert.ok(declared.has(name), `${combo.name}: ${name} is read but undeclared`);

    for (const c of creds) {
      const at = `${combo.name}: ${c.id}`;
      assert.ok(providers.has(c.provider), `${at}: provider ${c.provider} undeclared`);
      assert.ok(c.purpose && c.purpose !== "TODO", `${at}: purpose`);
      assert.ok(c.owner, `${at}: owner`);
      assert.equal(c.created_at, undefined, `${at}: created_at is for hoist system stamp`);
      assert.ok(c.consumers?.length, `${at}: consumers`);
      const runtimeOnly = c.consumers.every((u) => u.runtime);
      assert.ok(c.storage?.length || runtimeOnly, `${at}: storage`);
      assert.equal(c.rotation?.command, `hoist secrets rotate ${c.id}`, `${at}: rotation command`);
      assert.ok(c.rotation.steps?.length, `${at}: rotation steps`);
      if (c.role === "config" || c.expires_at === "never")
        assert.equal(c.max_ttl, undefined, `${at}: max_ttl with no expiry`);
      else {
        assert.ok(c.max_ttl && c.rotate_every, `${at}: max_ttl and rotate_every`);
        assert.equal(c.expires_at, undefined, `${at}: expires_at is for hoist system stamp`);
      }
      const keys = (c.storage ?? []).map((s) => s.key);
      const actual = new Set(keys.flatMap((k) => [...(jobs.get(k) ?? [])]));
      const listedJobs = c.consumers.filter((u) => u.workflow).map((u) => `${u.workflow}:${u.job}`);
      assert.deepEqual(new Set(listedJobs), actual, `${at}: consumers = jobs reading its keys`);
      for (const u of c.consumers.filter((u) => u.workflow))
        assert.equal(u.environment, envs.get(`${u.workflow}:${u.job}`), `${at}: env of ${u.job}`);
      for (const s of (c.storage ?? []).filter((s) => s.kind === "gh-env-secret"))
        assert.ok(
          c.consumers.some((u) => u.environment === s.env),
          `${at}: storage env ${s.env} has a consumer job`,
        );
      if (!actual.size)
        assert.ok(
          c.consumers.some((u) => u.runtime) || keys.some((k) => listed.has(k)),
          `${at}: unused`,
        );
      const registry = c.provider === "npm" || c.provider === "crates";
      if (registry) assert.notEqual(c.role, "publish", `${at}: publish is keyless`);
      if (c.provider === "github" && c.kind === "api_token")
        assert.ok(c.keyless_exempt, `${at}: a PAT needs keyless_exempt (App preferred)`);
      for (const t of sys.trust ?? [])
        if (t.provider === c.provider) assert.notEqual(t.replaces, c.role, `${at}: replaced`);
    }

    const trusts = (sys.trust ?? []).map((t) => `${t.provider}:${t.kind}`);
    for (const t of sys.trust ?? []) assert.ok(providers.has(t.provider), `trust ${t.provider}`);
    assert.ok(trusts.includes("github:github-app"), "GitHub App over PATs");
    const p = piecesOf(combo);
    const npm =
      comboTemplate(combo) === "template" && (p.includes("npm-cli") || p.includes("ts-lib"));
    assert.equal(trusts.includes("npm:oidc-trusted-publishing"), npm, "npm trusted publishing");
    const crates = comboTemplate(combo) === "template-rust" && p.includes("rust-cli");
    assert.equal(trusts.includes("crates:oidc-trusted-publishing"), crates, "crates trust");
    if (crates) {
      const first = creds.find((c) => c.provider === "crates");
      assert.ok(first?.keyless_exempt, "first crates.io publish: keyless_exempt");
      assert.equal(first.rotation.strategy, "assisted");
    }
    for (const id of ["circleci", "fly", "cloudflare"])
      assert.equal(
        providers.has(id),
        id === "cloudflare" ? comboTemplate(combo) === "template" : p.includes(id),
        `${combo.name}: provider ${id}`,
      );
    assert.match(text, /workload identity federation/i, "Google WIF hint");
  });
}

for (const tpl of ["template", "template-rust"]) {
  test(`${tpl}: AGENTS.md has one line for system.toml`, async () => {
    const { dir } = await render({
      name: "x",
      template: tpl,
      pieces: ["base"],
      vars: tpl === "template" ? VARS : {},
    });
    const lines = readFileSync(join(dir, "AGENTS.md"), "utf8").split("\n");
    assert.equal(lines.filter((l) => l.includes("system.toml")).length, 1);
  });
}

const WF = ".github/workflows/system-check.yml";
const BASE = join(HULL, "template/pieces/base/files");
const RUST = join(HULL, "template-rust/pieces/base/files");
const ROOT_WF = join(HULL, "../.github/workflows/system-check.yml");
const wfText = readFileSync(join(BASE, WF), "utf8");
const rustText = readFileSync(join(RUST, WF), "utf8");
const rootText = readFileSync(ROOT_WF, "utf8");
const TEMPLATES = { template: wfText, "template-rust": rustText };

for (const [tpl, text] of Object.entries(TEMPLATES)) {
  test(`system-check.yml (${tpl}): weekly, default branch, check reads, report writes issues only`, () => {
    const wf = YAML.parse(text);
    assert.deepEqual(Object.keys(wf.on).sort(), ["schedule", "workflow_dispatch"]);
    assert.equal(wf.on.schedule.length, 1);
    assert.match(wf.on.schedule[0].cron, /^\d{1,2} \d{1,2} \* \* \d$/);
    assert.deepEqual(wf.permissions, { contents: "read" });
    assert.deepEqual(Object.keys(wf.jobs), ["check", "report"]);
    for (const job of Object.values(wf.jobs)) {
      assert.match(job.if, /github\.event_name == 'schedule'/);
      assert.match(job.if, /github\.event_name == 'workflow_dispatch'/);
      assert.match(job.if, /default_branch/);
    }
    const { check, report } = wf.jobs;
    assert.deepEqual(check.permissions, { contents: "read" });
    assert.deepEqual(report.permissions, { issues: "write" });
    assert.equal(report.needs, "check");
    // The privileged job runs no repo code: no checkout, only the artifact, jq and gh.
    const uses = report.steps.filter((s) => s.uses).map((s) => s.uses.split("@")[0]);
    assert.deepEqual(uses, ["actions/download-artifact"]);
    for (const st of report.steps.filter((s) => s.run))
      assert.doesNotMatch(st.run, /\bnode\b|\.github\/|scripts\/|\.\/target|^\s*hoist /m, st.name);
    assert.ok(check.steps.some((s) => (s.uses ?? "").startsWith("actions/upload-artifact@")));
    assert.ok(!/secrets\./.test(text), "live tokens: github.token only");
    assert.match(text, /hoist system check --live --json/);
    assert.match(text, /npm install -g --ignore-scripts "@keelstackhq\/hoist@\$HOIST_VERSION"/);
    assert.deepEqual(ciMinutes(text), []);
  });
}

const hoistEnv = (text) =>
  YAML.parse(text).jobs.check.steps.find((s) => s.env?.HOIST_VERSION).env.HOIST_VERSION;

test("system-check.yml (template): HOIST_VERSION is a literal scripts/bump-version.sh moves", () => {
  assert.match(hoistEnv(wfText), /^\d+\.\d+\.\d+$/);
  assert.match(wfText, /bump-version\.sh keeps\n\s*# HOIST_VERSION in lockstep/);
});

test("system-check.yml (template-rust): hoist comes from pins.toml, at a release with `hoist system`", () => {
  assert.equal(hoistEnv(rustText), "${{ steps.pins.outputs.hoist }}");
  const steps = YAML.parse(rustText).jobs.check.steps;
  assert.equal(
    steps.find((s) => s.id === "pins").run,
    'node scripts/pins.mjs --github-output >> "$GITHUB_OUTPUT"',
  );
  const pin = parseToml(readFileSync(join(RUST, "pins.toml"), "utf8")).pins.hoist;
  const [maj, min] = pin.split(".").map(Number);
  assert.ok(maj > 0 || min >= 26, `pins.toml hoist ${pin} has no \`hoist system\` (0.26.0)`);
});

test("system-check.yml: the two templates differ only in where the hoist version comes from", () => {
  const strip = (t) => {
    const wf = YAML.parse(t);
    wf.jobs.check.steps = wf.jobs.check.steps.filter((s) => s.id !== "pins");
    for (const s of wf.jobs.check.steps) if (s.env?.HOIST_VERSION) s.env.HOIST_VERSION = "X";
    return wf;
  };
  assert.deepEqual(strip(rustText), strip(wfText));
});

test("system-check.yml: the root workflow behaves the same (title, label, concurrency, report, exits)", () => {
  const [root, tpl] = [rootText, wfText].map((t) => YAML.parse(t));
  assert.deepEqual(Object.keys(root.on).sort(), Object.keys(tpl.on).sort());
  assert.deepEqual(root.concurrency, tpl.concurrency);
  assert.deepEqual(root.permissions, tpl.permissions);
  assert.deepEqual(Object.keys(root.jobs), Object.keys(tpl.jobs));
  assert.equal(root.jobs.check.if, tpl.jobs.check.if);
  assert.deepEqual(root.jobs.check.permissions, tpl.jobs.check.permissions);
  const norm = (job) => ({
    ...job,
    steps: job.steps.map(({ uses, ...s }) => ({ ...s, uses: uses?.split("@")[0] })),
  });
  assert.deepEqual(norm(root.jobs.report), norm(tpl.jobs.report));
});

test("system-check.yml: every action pinned to a SHA another template workflow already uses", () => {
  const known = new Set();
  const walk = (d) =>
    readdirSync(d, { withFileTypes: true }).flatMap((e) =>
      e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)],
    );
  for (const f of [...walk(join(HULL, "template")), ...walk(join(HULL, "template-rust"))])
    if (/workflows\/(?!system-check)[^/]+\.ya?ml(\.jinja)?$/.test(f))
      for (const m of readFileSync(f, "utf8").matchAll(/uses: (\S+@[0-9a-f]{40}) (# v\S+)/g))
        known.add(`${m[1]} ${m[2]}`);
  for (const text of Object.values(TEMPLATES)) {
    const uses = [...text.matchAll(/uses: (\S+) (#.*)$/gm)].map((m) => `${m[1]} ${m[2]}`);
    assert.ok(uses.length >= 4);
    for (const u of uses) assert.ok(known.has(u), u);
  }
});

// Run a report-job step with a fake `gh` that logs its calls and keeps each --body-file.
const reportStep = (name) => YAML.parse(wfText).jobs.report.steps.find((s) => s.name === name);
function runStep(name, { env = {}, live, issues = [] }) {
  const dir = join(mkdtempSyncish(), "run");
  mkdirSync(join(dir, "bin"), { recursive: true });
  mkdirSync(join(dir, "system-check"), { recursive: true });
  if (live !== undefined)
    writeFileSync(
      join(dir, "system-check/live.json"),
      typeof live === "string" ? live : JSON.stringify(live),
    );
  writeFileSync(join(dir, "issues.json"), JSON.stringify(issues));
  writeFileSync(
    join(dir, "bin/gh"),
    `#!/usr/bin/env bash
printf '%s\\n' "$*" >> "${dir}/gh.log"
prev=""
for a in "$@"; do
  if [ "$prev" = "--body-file" ]; then cp "$a" "${dir}/body.md"; fi
  prev=$a
done
if [ "$1 $2" = "issue list" ]; then cat "${dir}/issues.json"; fi
`,
  );
  chmodSync(join(dir, "bin/gh"), 0o755);
  const step = reportStep(name);
  // The step's own env, with `${{ runner.temp }}` resolved; the test's env wins.
  const own = Object.fromEntries(
    Object.entries(step.env ?? {}).map(([k, v]) => [k, v.replace("${{ runner.temp }}", dir)]),
  );
  const r = spawnSync("bash", ["-c", step.run], {
    encoding: "utf8",
    env: {
      ...own,
      PATH: `${join(dir, "bin")}:${process.env.PATH}`,
      RUNNER_TEMP: dir,
      RUN_URL: "https://github.com/o/r/actions/runs/1",
      ...env,
    },
  });
  const read = (f) => (existsSync(join(dir, f)) ? readFileSync(join(dir, f), "utf8") : "");
  return { ...r, log: read("gh.log"), body: read("body.md") };
}
let tmpCount = 0;
function mkdtempSyncish() {
  const d = join(tmpdir(), `hull-syscheck-${process.pid}-${tmpCount++}`);
  mkdirSync(d, { recursive: true });
  return d;
}
const UPDATE = "Update the system-check issue";
const FAIL = "Fail on drift or a broken file";
const TITLE = "system.toml: drift or credentials to rotate";
const report = (items, steps = []) => ({
  version: 1,
  command: "system check",
  ok: false,
  exit_code: 1,
  items,
  human_steps: steps,
});
const drift = {
  area: "system",
  kind: "metadata",
  key: "a",
  action: "drift",
  reason: "missing created_at",
  source: "system.toml:9",
};
const okItem = { ...drift, kind: "credential", action: "ok", reason: "fine" };

test("system-check report: drift creates the labelled issue with the findings only", () => {
  const r = runStep(UPDATE, { env: { EXIT: "1", OFFLINE: "1" }, live: report([drift, okItem]) });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.log, /^label create system-check --color fbca04 /m);
  assert.match(
    r.log,
    new RegExp(`^issue create --title ${TITLE} --label system-check --body-file `, "m"),
  );
  assert.match(r.body, /missing created_at/);
  assert.doesNotMatch(r.body, /fine/);
});

test("system-check report: neutralizes @mentions and backticks, keeps only plain https step urls", () => {
  const evil = { ...drift, key: "@org/team", reason: "x `rm` | y\nz @someone" };
  const steps = [
    { id: "a", title: "Rotate @admin", url: "https://dash.cloudflare.com/profile/api-tokens" },
    { id: "b", title: "Bad", url: "javascript:alert(1)" },
    { id: "c", title: "Sneaky", url: "https://x.example/) [click](https://evil" },
  ];
  const r = runStep(UPDATE, { env: { EXIT: "2", OFFLINE: "2" }, live: report([evil], steps) });
  assert.equal(r.status, 0, r.stderr);
  assert.doesNotMatch(r.body, /(^|[^​])@(org|someone|admin)/);
  assert.match(r.body, /@​org/);
  assert.doesNotMatch(
    r.body
      .split("\n")
      .filter((l) => l.startsWith("|"))
      .join("\n"),
    /`/,
  );
  assert.match(r.body, /<https:\/\/dash\.cloudflare\.com\/profile\/api-tokens>/);
  assert.doesNotMatch(r.body, /javascript:|evil/);
});

test("system-check report: updates the lowest open issue, closes it when clean, no-op when none", () => {
  const issues = [
    { number: 9, title: TITLE },
    { number: 2, title: "other" },
    { number: 4, title: TITLE },
  ];
  const u = runStep(UPDATE, { env: { EXIT: "1", OFFLINE: "1" }, live: report([drift]), issues });
  assert.equal(u.status, 0, u.stderr);
  assert.match(u.log, /^issue edit 4 --title /m);
  const c = runStep(UPDATE, { env: { EXIT: "0", OFFLINE: "0" }, live: report([okItem]), issues });
  assert.equal(c.status, 0, c.stderr);
  assert.match(c.log, /^issue close 4 --comment /m);
  const n = runStep(UPDATE, { env: { EXIT: "0", OFFLINE: "0" }, live: report([]) });
  assert.equal(n.status, 0, n.stderr);
  assert.doesNotMatch(n.log, /issue (create|edit|close)/);
});

test("system-check report: no report or exit 3 still opens the issue; long bodies are cut", () => {
  const p = runStep(UPDATE, {
    env: { EXIT: "3", OFFLINE: "3" },
    live: "error: unknown key `x` @y",
  });
  assert.equal(p.status, 0, p.stderr);
  assert.match(p.log, /^issue create /m);
  assert.match(p.body, /exited 3 without a JSON report/);
  assert.doesNotMatch(p.body, /unknown key/);
  const many = Array.from({ length: 3000 }, (_, i) => ({
    ...drift,
    key: `k${i}`,
    reason: "x".repeat(40),
  }));
  const l = runStep(UPDATE, { env: { EXIT: "1", OFFLINE: "1" }, live: report(many) });
  assert.equal(l.status, 0, l.stderr);
  assert.ok(Buffer.byteLength(l.body) < 65536, `${Buffer.byteLength(l.body)} bytes`);
  assert.match(l.body, /truncated/);
});

test("system-check report: exit 0 and 2 pass, metadata-only drift warns, other drift and 3 fail", () => {
  const run = (code, live) => runStep(FAIL, { env: { EXIT: code }, live });
  assert.equal(run("0", report([])).status, 0);
  assert.equal(run("2", report([{ ...drift, kind: "expiry", action: "pending" }])).status, 0);
  const meta = run("1", report([drift, okItem]));
  assert.equal(meta.status, 0);
  assert.match(meta.stdout, /::warning::/);
  assert.equal(
    run("1", report([drift, { ...drift, kind: "expiry", reason: "expired 3 days ago" }])).status,
    1,
  );
  assert.equal(run("1", "not json").status, 1);
  assert.equal(run("3", report([])).status, 1);
});
