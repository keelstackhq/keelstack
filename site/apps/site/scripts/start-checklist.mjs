// Copies hull's docs/start-checklist.md at a ref into src/data/hull/ (with its source and sha256)
// and writes the "Start a project" page from it. Usage: node scripts/start-checklist.mjs <ref>
// (a tag or a commit; the source records the commit). The test start-checklist.test.ts checks both.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";

const ref = process.argv[2];
if (!ref) throw new Error("usage: node scripts/start-checklist.mjs <hull tag or commit>");
const repo = "keelstackhq/hull";
const path = "docs/start-checklist.md";

const line = execFileSync("git", ["ls-remote", `https://github.com/${repo}`, ref, `${ref}^{}`], {
  encoding: "utf8",
})
  .trim()
  .split("\n")
  .filter(Boolean)
  .pop();
const commit = /^[0-9a-f]{40}$/.test(ref) ? ref : line?.split("\t")[0];
if (!commit) throw new Error(`no hull ref ${ref}`);

const res = await fetch(`https://raw.githubusercontent.com/${repo}/${commit}/${path}`);
if (!res.ok) throw new Error(`fetch ${path}@${commit}: ${res.status}`);
const copy = await res.text();
const sha256 = createHash("sha256").update(copy).digest("hex");

const dir = new URL("../src/data/hull/", import.meta.url);
writeFileSync(new URL("start-checklist.md", dir), copy);
writeFileSync(
  new URL("start-checklist.json", dir),
  `${JSON.stringify({ repo, ref: commit, path, sha256 }, null, 2)}\n`,
);

const isCommit = /^[0-9a-f]{40}$/.test(ref);
const label = isCommit ? `commit \`${commit.slice(0, 7)}\`` : `\`${ref}\``;
// "(not tagged yet)" only while hull has no v0.5.0 tag: ask the remote, never assume.
const sinceTag = "v0.5.0";
const tagged = execFileSync(
  "git",
  ["ls-remote", "--tags", `https://github.com/${repo}`, sinceTag],
  {
    encoding: "utf8",
  },
).trim();
const since = tagged ? `hull ${sinceTag}` : `hull ${sinceTag} (not tagged yet)`;
const NOTE = `

---

Note on this copy (not part of hull's file): it is pinned to hull v0.11.0. Its janitor routine is
optional on bosun \`main\` after 0.15.0 and on hull \`main\`: set \`[routines] janitor = true\` only if you want it. Where
it names a bosun version (such as "bosun 0.4.0"), use the bosun version in the versions table
(\`apps/site/src/data/versions.ts\`, the footer's verified line) instead.
`;
const page = `---
layout: ../../layouts/Docs.astro
---

This page is hull's
[\`docs/start-checklist.md\`](https://github.com/${repo}/blob/${commit}/${path}) at ${label},
copied verbatim. A repo rendered from ${since} or later carries the same list: its \`ready\` skill
(\`node scripts/ready.mjs\`) runs the checks and links each item to this file at the hull tag the
repo was rendered from. The full path, with the details, is in
[Getting started](/docs/getting-started/); for a repo that bosun runs, see
[Greenfield repo with bosun](/docs/greenfield-bosun/). A repo that already has code follows
[Adopt an existing repo](/docs/adopt/).

\`TODO(owner)\` in the copy is hull's own marker for facts hull's maintainers have not written down. It is not a step for you: skip it.

${copy.replace(/^# .*\n\n/, "")}${NOTE}`;
writeFileSync(new URL("../src/pages/docs/start-project.md", import.meta.url), page);
console.log(`start-checklist: ${repo}@${commit} sha256 ${sha256}`);
