// The agent doc (agents/README.md, served at /agents.md) is read in full by an AI agent working in a
// TARGET repo. Step 0, the facts it relies on, each with its source at the pinned tag (checked 2026-11-16):
//  1. bosun prints one JSON object {version, command, ok, exit_code, data, error, next}; error has
//     {kind, message, fix}; exit codes 0 ok, 1 error, 2 usage, 3 conflict, 4 halted, 5 needs-human,
//     6 limit. Source: `bosun --help`, bosun at versions.bosun (also checked at 0.18.0).
//  2. `bosun prereqs <plan files>` lists the human steps a plan needs and its approvals (--post opens
//     the checklist issue). Source: `bosun prereqs --help` at versions.bosun (0.18.0; same text at 0.17.0).
//  3. `bosun doctor` reports each checklist step ok or missing; `bosun keygen --enroll` adds a key to
//     .bosun/approvers.toml; `bosun migrate --to <mode> [--dry-run|--direct]`, `--finish` after the
//     merge. Source: `bosun doctor|keygen|migrate --help` at versions.bosun.
//  4. `splice check` exits 1 on owned drift, leftover conflict markers OR an available update.
//     Source: `splice check --help` at versions.splice (0.6.0; same text at 0.5.1).
//  5. `hoist check` is plan plus verification, read-only, exit 1 on drift; `hoist apply` writes then
//     verifies. Source: `hoist --help`, `hoist check --help` at versions.hoist.
//  6. `--assess --json` of hull's ready.mjs has schema `hull.assess/1` with fields template
//     (template|template-rust|null), stack (ts-pnpm|rust|null), confidence, unsure, pieces, signals,
//     clashes, next. Source: hull docs/adopt.md at versions.hull (src/data/hull/adopt.md).
//  7. `bosun doctor` at versions.bosun: step 2 "Claude GitHub App", step 6 "setup script and version"
//     (checks the repo file docs/setup-script.sh, not the paste). No step checks an approver key:
//     the key is enrolled in .bosun/approvers.toml (`bosun keygen --enroll --help`).
//  8. `stack` is null whenever `template` is null (hull adopt.md); channels only is
//     `splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project`
//     (adopt.md step 10; flags in `splice channel add --help`); `hoist plan|apply|check --adopt`
//     exist at versions.hoist.
//  9. Unverified, so TODO(copy): a machine check that the cloud environment's setup script was pasted.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";
import { buildLlmsTxt } from "../lib/llms.ts";
import { buildCtx } from "../lib/render.ts";
import { HUMAN_STEPS } from "./scenarios.ts";
import { docRefs, versions } from "./versions.ts";

const site = join(dirname(fileURLToPath(import.meta.url)), "../..");
const repo = join(site, "../..");
const doc = readFileSync(join(repo, "agents/README.md"), "utf8");
const adopt = JSON.parse(readFileSync(join(site, "src/data/hull/adopt.json"), "utf8")) as {
  assess: { url: string; sha256: string };
};
const ctx = buildCtx(undefined, {
  ...docRefs,
  assessUrl: adopt.assess.url,
  assessSha: adopt.assess.sha256,
});

const fenced = (s: string): string[] =>
  [...s.matchAll(/```bash\n([\s\S]*?)\n```/g)].map((m) => m[1]!);

/** Opening fences that are not exactly ```bash (fences alternate open, close). */
const openers = (s: string): string[] =>
  (s.match(/^```.*$/gm) ?? []).filter((l, i) => i % 2 === 0 && l !== "```bash");

describe("agents/README.md", () => {
  test("every bash block equals a builder output", () => {
    const known = new Set(
      Object.entries(ctx)
        .filter(([k]) => k.startsWith("block."))
        .flatMap(([, v]) => fenced(v)),
    );
    const blocks = fenced(doc);
    expect(blocks.length).toBeGreaterThan(5);
    expect(blocks.filter((b) => !known.has(b))).toEqual([]);
    expect(ctx["block.verifyBlock"]).toBeDefined();
    expect(ctx["block.rollbackBlock"]).toBeDefined();
  });

  test("every opening fence is exactly ```bash", () => {
    expect(openers(doc)).toEqual([]);
    expect(openers("```bash\nx\n```\n```\ny\n```\n")).toEqual(["```"]);
    expect(openers("```js\nx\n```\n")).toEqual(["```js"]);
  });

  test("agents/versions.json equals versions.ts", () => {
    const json = JSON.parse(readFileSync(join(repo, "agents/versions.json"), "utf8")) as {
      versions: unknown;
    };
    expect(json.versions).toEqual(versions);
  });

  test("stays at or under 10 KiB", () => {
    expect(new TextEncoder().encode(doc).length).toBeLessThanOrEqual(10240);
  });

  test("has the required headings", () => {
    for (const h of ["Decide", "Human-only", "Steps", "Verify", "Rollback", "Sources"])
      expect(doc).toMatch(new RegExp(`^## .*${h}`, "m"));
  });

  test("states the versions from versions.ts and the bosun JSON contract", () => {
    for (const v of Object.values(versions)) expect(doc).toContain(v);
    expect(doc).toContain("error.fix");
    expect(doc).toContain("`next`");
    expect(doc).toContain("5 needs-human");
  });

  test("names no client and leaves gaps as TODO(copy)", () => {
    expect(doc).toContain("TODO(copy)");
    expect(doc).not.toMatch(/Opus|Sonnet|Haiku/);
  });
});

const start = readFileSync(join(site, "src/pages/docs/start.md"), "utf8");
const sect = (from: RegExp, to: RegExp): string => {
  const a = doc.search(from);
  const rest = doc.slice(a + 1);
  return doc.slice(a, a + 1 + rest.search(to));
};

describe("agents/README.md, review round 1b", () => {
  test("Decide: C is checked before D (explicit tie-break), and bosun alone is reachable", () => {
    const decide = sect(/^## Decide/m, /^## Human/m);
    expect(decide.indexOf("path C")).toBeGreaterThan(-1);
    expect(decide.indexOf("path C")).toBeLessThan(decide.indexOf("path D"));
    expect(decide).toMatch(/\.bosun\/version[^\n]*mode change[^\n]*path C/);
    expect(decide).toContain("before D");
    expect(decide).toContain("bosun adopt");
  });

  test("human steps: one data source, in order, rendered in both the agent doc and /docs/start", () => {
    let at = -1;
    for (const s of HUMAN_STEPS) {
      const i = doc.indexOf(s.what);
      expect(i, s.what).toBeGreaterThan(at);
      at = i;
      expect(start).toContain(s.what);
    }
    expect(doc).not.toMatch(/\| *Create and push/);
    const keygen = HUMAN_STEPS.findIndex((s) => s.what.includes("keygen --enroll"));
    const merge = HUMAN_STEPS.findIndex((s) => s.what.includes("setup PR"));
    expect(keygen).toBeGreaterThan(-1);
    expect(keygen).toBeLessThan(merge);
    expect(start).not.toMatch(/merge the setup PR yourself|merge it yourself/);
  });

  test("the plan's source is stated on A, B and C; pnpm is a prerequisite", () => {
    expect(doc.match(/owner's brief/g)?.length).toBeGreaterThanOrEqual(3);
    expect(doc).toMatch(/Needs[^\n]*pnpm/);
    expect(doc).not.toContain("npx ");
    expect(doc).toContain("pnpm dlx");
  });

  test("C: bosun pr --ready only with --no-ready; verify after adopt, migrate", () => {
    expect(doc).toMatch(/--no-ready[\s\S]*bosun pr --pr <n> --ready/);
    expect(doc).toMatch(/--finish[^\n]*\n[^\n]*bosun doctor/);
    expect(doc).toContain("splice@" + versions.splice.replace(/^v/, "") + " check --offline");
    expect(doc).toContain("keelstack.dev/docs/start/#e");
    expect(doc).not.toMatch(/paste: TODO/);
  });
});

test("/llms.txt links /agents.md and stays under 3.5 KB", () => {
  const out = buildLlmsTxt();
  expect(out).toContain("https://keelstack.dev/agents.md");
  expect(out.length).toBeLessThan(3500);
});

test("the built dist/agents.md is the committed file", () => {
  const built = new URL("../../dist/agents.md", import.meta.url);
  expect(existsSync(built), "apps/site/dist is missing: build the site first").toBe(true);
  expect(readFileSync(built, "utf8")).toBe(doc);
});
