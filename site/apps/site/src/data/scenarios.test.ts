import { readFileSync } from "node:fs";
import { expect, test } from "vite-plus/test";
import { buildLlmsTxt } from "../lib/llms";
import { pickPath } from "./content";
import {
  ADOPT_BOSUN_OWNER,
  ADOPT_HULL_NOTE,
  adoptBosunBlock,
  adoptHullBlock,
  installBlock,
  APPROVE_COMMENT,
  cliBlock,
  CLOUD_TOML,
  MIGRATE_COMMANDS,
  newRepoBlock,
  NETWORK,
  hoistBlock,
  upgradeBlock,
  UPGRADE_NOTE,
  approveBlock,
  SIGNED_PREREQ,
  NEW_REPO_OWNER,
  NO_APPROVAL,
  OWNER_APPROVES,
  SETUP_SHORT,
  SIGNER_STEPS,
  SLACK_COMMANDS,
  SLACK_NOTIFY_DIGEST,
  SLACK_TOML,
  scenarios,
} from "./scenarios";
import { bare, versions } from "./versions";

const read = (rel: string) => readFileSync(new URL(`../pages/${rel}`, import.meta.url), "utf8");
const start = read("docs/start.md");
const risk = read("docs/risk.md");
const cloud = read("docs/cloud.md");
const adopt = read("docs/adopt.md");
const adoptSrc = JSON.parse(
  readFileSync(new URL("./hull/adopt.json", import.meta.url), "utf8"),
) as { assess: { url: string; sha256: string } };

const lines = (block: string) => block.split("\n");

test("scenarios a to f, each with an anchor on /docs/start and a landing card", () => {
  expect(scenarios.map((s) => s.id)).toEqual(["a", "b", "c", "d", "e", "f"]);
  for (const s of scenarios) expect(start).toContain(`<a id="${s.id}"></a>`);
  expect(pickPath.cards.map((c) => c.href)).toEqual(scenarios.map((s) => `/docs/start/#${s.id}`));
});

test("every code block of /docs/start is at most 12 lines", () => {
  for (const block of start.split("```").filter((_, i) => i % 2 === 1)) {
    expect(block.trim().split("\n").length - 1).toBeLessThanOrEqual(12);
  }
});

const section = (id: string): string => {
  const from = start.indexOf(`<a id="${id}"></a>`);
  const next = start.slice(from + 1).search(/<a id="/);
  return next < 0 ? start.slice(from) : start.slice(from, from + 1 + next);
};

test("each /docs/start block sits in its own scenario section, whole and in order", () => {
  const want: [string, string][] = [
    ["a", newRepoBlock(versions)],
    ["b", adoptHullBlock(versions, adoptSrc.assess.url, adoptSrc.assess.sha256)],
    ["b", adoptBosunBlock],
    [
      "c",
      MIGRATE_COMMANDS.split("\n")
        .map((l) => l.split("   #")[0])
        .join("\n"),
    ],
    ["e", cliBlock],
    ["e", SETUP_SHORT],
    ["e", SLACK_TOML],
    ["e", SLACK_COMMANDS],
  ];
  for (const [id, block] of want) {
    const body = section(id);
    if (id === "c") {
      for (const l of block.split("\n")) expect(body).toContain(l);
    } else {
      expect(body).toContain(block);
    }
  }
});

test("/docs/start carries the shared blocks", () => {
  const blocks = [
    newRepoBlock(versions),
    hoistBlock(versions),
    adoptHullBlock(versions, adoptSrc.assess.url, adoptSrc.assess.sha256),
    adoptBosunBlock,
    cliBlock,
    SETUP_SHORT,
    SLACK_TOML,
    SLACK_COMMANDS,
    upgradeBlock(versions),
    approveBlock,
  ];
  for (const b of blocks) for (const l of lines(b)) expect(start).toContain(l);
  for (const l of lines(MIGRATE_COMMANDS)) expect(start).toContain(l.split("   #")[0]);
  for (const t of [
    NEW_REPO_OWNER,
    ADOPT_BOSUN_OWNER,
    UPGRADE_NOTE,
    SIGNED_PREREQ,
    NETWORK,
    NO_APPROVAL,
  ]) {
    expect(start).toContain(t.replace(/`/g, ""));
  }
  expect(start).toContain(APPROVE_COMMENT.replace("<40-character head SHA>", ""));
});

test("/docs/adopt leads with the same assess, splice adopt and bosun adopt blocks", () => {
  const head = adopt.split("## Use Claude")[0] ?? "";
  for (const b of [
    adoptHullBlock(versions, adoptSrc.assess.url, adoptSrc.assess.sha256),
    adoptBosunBlock,
  ]) {
    expect(head).toContain(b);
  }
});

test("/docs/cloud links the setup script and network note (one copy, in /docs/start) and has the [cloud] table", () => {
  expect(cloud).not.toContain(SETUP_SHORT);
  expect(cloud).not.toContain(NETWORK);
  expect(cloud).toContain("/docs/start/#e");
  expect(cloud).toContain(CLOUD_TOML);
});

test("/docs/risk has the cheat sheet, the 4 signer steps and the digest", () => {
  expect(risk).toContain(NO_APPROVAL);
  expect(risk).toContain(OWNER_APPROVES);
  expect(risk).toContain(APPROVE_COMMENT.replace("<40-character head SHA>", ""));
  expect(SIGNER_STEPS).toHaveLength(4);
  const listed = SIGNER_STEPS.map(
    (step, i) => `${i + 1}. ${step.startsWith("bosun ") ? `\`${step}\`` : step}`,
  );
  expect(risk).toContain(listed.join("\n"));
  expect(risk).toContain(SLACK_NOTIFY_DIGEST);
});

test("llms.txt: one line per scenario and one version line", () => {
  const out = buildLlmsTxt();
  for (const s of scenarios) expect(out).toContain(`- ${s.id.toUpperCase()}. ${s.title}:`);
  expect(out).toContain(
    `Current versions: hull ${versions.hull}, splice ${versions.splice}, bosun ${versions.bosun}, hoist ${versions.hoist}, hoard ${versions.hoard}, berth ${versions.berth}.`,
  );
});

test("no docs page that pins a version names an older hull, bosun or hoist", () => {
  const pages = [
    "start",
    "start-here",
    "getting-started",
    "greenfield-bosun",
    "cloud",
    "adopt",
    "index",
  ];
  for (const p of pages) {
    const text = read(`docs/${p}.md`);
    for (const m of text.matchAll(/--ref (v\d+\.\d+\.\d+)|render hull (v\d+\.\d+\.\d+)/g)) {
      expect(m[1] ?? m[2], p).toBe(versions.hull);
    }
    for (const m of text.matchAll(/@keelstackhq\/bosun@(\d+\.\d+\.\d+)/g))
      expect(m[1], p).toBe(bare(versions.bosun));
    for (const m of text.matchAll(/@keelstackhq\/hoist@(\d+\.\d+\.\d+)/g))
      expect(m[1], p).toBe(versions.hoist);
    for (const m of text.matchAll(
      /Latest releases as of [\d-]+: hull (v\d+\.\d+\.\d+), splice ([\d.]+), bosun ([\d.]+)/g,
    )) {
      expect([m[1], m[2], m[3]], p).toEqual([versions.hull, versions.splice, bare(versions.bosun)]);
    }
  }
  expect(read("docs/start.md")).toContain(
    `hull ${versions.hull}, splice ${versions.splice}, bosun ${bare(versions.bosun)}`,
  );
});

test("the bosun install line is on /docs/start once, in the setup block", () => {
  expect(start).toContain(installBlock(versions));
  expect(start.split(`npm i -g @keelstackhq/bosun@${bare(versions.bosun)}`)).toHaveLength(2);
  expect(start.split("export GITHUB_TOKEN=$(gh auth token)").length).toBeLessThanOrEqual(3);
});

test("/docs/adopt carries the same B notes as /docs/start", () => {
  const adoptPage = read("docs/adopt.md");
  expect(adoptPage).toContain(ADOPT_HULL_NOTE);
  expect(adoptPage).toContain(ADOPT_BOSUN_OWNER);
});

test("block A commits the render before it pushes (splice new only runs git init)", () => {
  const b = newRepoBlock(versions);
  const commit = b.indexOf('git add -A && git commit -m "chore: render from hull"');
  expect(commit).toBeGreaterThan(-1);
  expect(commit).toBeLessThan(b.indexOf("gh repo create"));
});
