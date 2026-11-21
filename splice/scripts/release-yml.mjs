// Test helpers for the monorepo's root .github/workflows/release.yml (D-023), which now tags,
// releases and moves the Action's major tag for splice: read a job, extract a step's script, run
// it under bash against a fake `gh` and real git repositories.
import { execFileSync, spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const releaseYml = () =>
  readFileSync(new URL("../../.github/workflows/release.yml", import.meta.url), "utf8");

/** The text of top-level job `name`, up to the next job. */
export function job(yml, name) {
  const m = yml.match(new RegExp(`^  ${name}:\\n([\\s\\S]*?)(?=^  [a-z-]+:\\n|(?![\\s\\S]))`, "m"));
  if (!m) throw new Error(`job ${name} not found`);
  return m[1];
}

/** The dedented `run: |` script of the step named `step` in job text `jobText`. */
export function stepScript(jobText, step) {
  const at = jobText.indexOf(`      - name: ${step}\n`);
  if (at < 0) throw new Error(`step ${step} not found`);
  const rest = jobText.slice(at);
  const run = rest.indexOf("        run: |\n");
  if (run < 0) throw new Error(`step ${step} has no run: | script`);
  const out = [];
  for (const line of rest.slice(run + "        run: |\n".length).split("\n")) {
    if (line !== "" && !line.startsWith("          ")) break;
    out.push(line.slice(10));
  }
  return out.join("\n") + "\n";
}

// A fake `gh` for the release API calls release.yml makes (tags are git's: they are pushed over
// SSH, which tests point at a bare origin with url.insteadOf). State (JSON): `releases`
// [{id, tag, draft}], `next` (release id), `fail` (an endpoint prefix that errors). Every call is
// logged as one JSON array per line.
const FAKE_GH = String.raw`#!/usr/bin/env node
const fs = require("fs");
const file = process.env.FAKE_GH_STATE;
const s = JSON.parse(fs.readFileSync(file, "utf8"));
const a = process.argv.slice(2);
fs.appendFileSync(process.env.FAKE_GH_LOG, JSON.stringify(a) + "\n");
const save = () => fs.writeFileSync(file, JSON.stringify(s));
const flag = (n) => { const i = a.indexOf(n); return i < 0 ? undefined : a[i + 1]; };
const fields = {};
a.forEach((x, i) => {
  if (x === "-f" || x === "-F") { const [k, ...v] = a[i + 1].split("="); fields[k] = v.join("="); }
});
const fail = (msg) => { process.stderr.write(msg + "\n"); process.exit(1); };
if (a[0] === "release" && a[1] === "create") {
  s.releases.push({ id: s.next++, tag: a[2], draft: a.includes("--draft") });
  save();
  process.exit(0);
}
if (a[0] !== "api") fail("fake gh: unsupported " + a.join(" "));
const method = flag("-X") || "GET";
const path = a.slice(1).find((x) => x.startsWith("repos/")).replace(/^repos\/[^/]+\/[^/]+\//, "");
if (s.fail && path.startsWith(s.fail)) fail("HTTP 500: " + path);
let m;
if (method === "GET" && path.startsWith("releases?")) {
  for (const r of s.releases)
    if (r.tag === process.env.TAG) console.log(r.id + " " + (r.draft ? "draft" : "published"));
} else if ((m = path.match(/^releases\/(\d+)$/))) {
  const r = s.releases.find((x) => x.id === Number(m[1]));
  if (!r) fail("HTTP 404");
  if (method === "PATCH") r.draft = fields.draft !== "false";
  if (method === "DELETE") s.releases = s.releases.filter((x) => x !== r);
  save();
} else fail("fake gh: unsupported " + method + " " + path);
`;

/** A directory with the fake `gh` and its state. */
export function fakeGh(state = {}) {
  const dir = mkdtempSync(join(tmpdir(), "fake-gh-"));
  const bin = join(dir, "bin");
  execFileSync("mkdir", ["-p", bin]);
  writeFileSync(join(bin, "gh"), FAKE_GH);
  chmodSync(join(bin, "gh"), 0o755);
  const stateFile = join(dir, "state.json");
  const logFile = join(dir, "log");
  writeFileSync(stateFile, JSON.stringify({ releases: [], next: 1, ...state }));
  writeFileSync(logFile, "");
  return {
    dir,
    env: { PATH: `${bin}:${process.env.PATH}`, FAKE_GH_STATE: stateFile, FAKE_GH_LOG: logFile },
    state: () => JSON.parse(readFileSync(stateFile, "utf8")),
    calls: () =>
      readFileSync(logFile, "utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l)),
  };
}

/** An isolated git environment: no global or system config, fixed author. */
export function gitEnv(dir) {
  const env = {
    GIT_CONFIG_GLOBAL: join(dir, "gitconfig"),
    GIT_CONFIG_NOSYSTEM: "1",
    GIT_AUTHOR_NAME: "t",
    GIT_AUTHOR_EMAIL: "t@example.com",
    GIT_COMMITTER_NAME: "t",
    GIT_COMMITTER_EMAIL: "t@example.com",
  };
  writeFileSync(env.GIT_CONFIG_GLOBAL, "[init]\n\tdefaultBranch = main\n");
  return env;
}

/** Run `script` under bash in `cwd` with `env` over the process env; GITHUB_OUTPUT/ENV files. */
export function runStep(script, { cwd, env }) {
  const out = join(mkdtempSync(join(tmpdir(), "step-")), "out");
  writeFileSync(out, "");
  const r = spawnSync("bash", ["-e", "-c", script], {
    cwd,
    env: { ...process.env, GITHUB_OUTPUT: out, GITHUB_ENV: out, ...env },
    encoding: "utf8",
  });
  return { ...r, output: readFileSync(out, "utf8") };
}
