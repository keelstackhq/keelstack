// Copies hull's docs/adopt.md and the assessment (template/pieces/base/files/scripts/ready.mjs) at
// a ref: the doc to src/data/hull/adopt.md, the script to src/data/hull/ready.mjs.txt (served at
// /assess/<tag>.mjs by src/pages/assess/[file].ts), both recorded
// with their sha256 in src/data/hull/adopt.json, and writes the "Adopt an existing repo" page.
// Usage: node scripts/adopt.mjs <hull tag or commit> <assess tag, e.g. v0.6.0> [--releasing]
// (--releasing marks hull <tag> and splice 0.4.0 as releasing now). adopt.test.ts checks it all.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { writeFileSync } from "node:fs";

const [ref, tag, flag] = process.argv.slice(2);
if (!ref || !/^v\d+\.\d+\.\d+$/.test(tag ?? "")) {
  throw new Error("usage: node scripts/adopt.mjs <hull tag or commit> <vX.Y.Z> [--releasing]");
}
const releasing = flag === "--releasing";
const repo = "keelstackhq/hull";
const docPath = "docs/adopt.md";
const assessPath = "template/pieces/base/files/scripts/ready.mjs";

const isCommit = /^[0-9a-f]{40}$/.test(ref);
const commit = isCommit
  ? ref
  : execFileSync("git", ["ls-remote", `https://github.com/${repo}`, ref, `${ref}^{}`], {
      encoding: "utf8",
    })
      .trim()
      .split("\n")
      .filter(Boolean)
      .pop()
      ?.split("\t")[0];
if (!commit) throw new Error(`no hull ref ${ref}`);

async function fetchText(path) {
  const res = await fetch(`https://raw.githubusercontent.com/${repo}/${commit}/${path}`);
  if (!res.ok) throw new Error(`fetch ${path}@${commit}: ${res.status}`);
  return res.text();
}
const sha256 = (s) => createHash("sha256").update(s).digest("hex");
const doc = await fetchText(docPath);
const assess = await fetchText(assessPath);
const url = `/assess/${tag}.mjs`;

const data = new URL("../src/data/hull/", import.meta.url);
writeFileSync(new URL("adopt.md", data), doc);
writeFileSync(
  new URL("adopt.json", data),
  `${JSON.stringify(
    {
      repo,
      ref: commit,
      doc: { path: docPath, sha256: sha256(doc) },
      assess: { path: assessPath, url, sha256: sha256(assess) },
    },
    null,
    2,
  )}\n`,
);
// Served at `url` by src/pages/assess/[file].ts; .txt keeps lint and fmt off the verbatim copy.
writeFileSync(new URL("ready.mjs.txt", data), assess);

const pending = releasing
  ? `\nUntil hull ${tag} and splice 0.4.0 are tagged, step 1 works but steps 3 and 4 do not yet:
stop after step 2 and tell me.`
  : "";
const note = releasing
  ? ` hull ${tag} and splice 0.4.0 are releasing now; until they are tagged, this follows hull's main.`
  : "";
const page = `---
layout: ../../layouts/Docs.astro
---

The guide below is hull's
[\`docs/adopt.md\`](https://github.com/${repo}/blob/${commit}/${docPath}) at commit
\`${commit.slice(0, 7)}\`, copied verbatim.${note} Its \`start-checklist.md\` is
[Start a project](/docs/start-project/) on this site.

## Use Claude

Paste this into Claude Code in the repo you want to adopt. It runs the read-only assessment
first and asks you before anything that writes. The assessment is hull's \`ready.mjs\`, hosted
at \`https://keelstack.dev${url}\` with this sha256:
\`${sha256(assess)}\`.

\`\`\`text
Adopt this repo into keelstack. Work step by step, and ask me before anything that writes.${pending}

1. Assess (read-only). Stop if the checksum does not match:
   curl -fsSL https://keelstack.dev${url} -o /tmp/assess.mjs
   echo "${sha256(assess)}  /tmp/assess.mjs" | sha256sum -c
   node /tmp/assess.mjs --assess --json
2. Explain the result in plain words: the template, the pieces with their names and moves, the
   unsure notes and the clashes. Then ask me with AskUserQuestion forms: the template, the
   pieces and their names, which of my existing CI and config files to keep, and the channels
   (the decisions channel; bosun only if the stack is rust or ts-pnpm). Never decide an unsure
   item yourself. Tell me that keeping an owned base file (for example ci.yml) blocks adopt
   without --force: keeping one means channels only, or adopting it later (adopt.md).
3. Create a new branch with a clean working tree, move code into place as I confirmed, then run
   npx @keelstackhq/splice@0.4.0 adopt gh:keelstackhq/hull//<template> --ref ${tag} --piece <id>:<name> ... --dry-run --json
   and show me the change list by action: refuse, replace, merge, conflict, create, skip.
4. Only after I confirm: run the same command without --dry-run, resolve the conflicts, run the
   ready skill, and open a pull request with the change list and my choices. Never pass --force
   unless I named the files it may replace.
5. After the pull request merges: add the decisions channel, run bosun init if the stack is
   rust or ts-pnpm and I chose bosun, and ask me to run hoist apply, as in the start checklist.
\`\`\`

## The guide

${doc.replace(/^# .*\n\n/, "")}`;
writeFileSync(new URL("../src/pages/docs/adopt.md", import.meta.url), page);
console.log(`adopt: ${repo}@${commit} doc ${sha256(doc)} assess ${url} ${sha256(assess)}`);
