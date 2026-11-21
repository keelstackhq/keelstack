import { readyDocs } from "../data/docs";
import { hero, quickstart } from "../data/content";
import { tools } from "../data/tools";
import { scenarios } from "../data/scenarios";
import { versions } from "../data/versions";
import { docMdPath } from "./docs-md";
import { startMdPath } from "./start-md";
import { SITE_URL } from "./site";

// An index, not a copy: every exact command block lives once, in /docs/start.md.
export function buildLlmsTxt(): string {
  const toolLines = tools.map((t) => `[${t.name}](${t.repo})`).join(", ");
  const docLines = readyDocs().map((d) => `- [${d.title}](${SITE_URL}${docMdPath(d.slug)})`);
  const fields = quickstart.agents.fields.map((f) => `\`${f}\``).join(", ");
  const codes = quickstart.agents.exitCodes.map(([n, name]) => `${n} ${name}`).join(", ");
  return [
    "# keelstack",
    "",
    `> ${hero.body}`,
    "",
    "## Start here",
    "",
    `If you are an agent in a repo to set up, adopt or upgrade: read ${SITE_URL}/agents.md in full first (one file: decide, human-only steps, exact steps, verify, rollback).`,
    "",
    `Current versions: hull ${versions.hull}, splice ${versions.splice}, bosun ${versions.bosun}, hoist ${versions.hoist}, hoard ${versions.hoard}, berth ${versions.berth}. Each scenario file below is Setup plus one exact copy-paste block: read only yours.`,
    "",
    ...scenarios.map(
      (s) => `- ${s.id.toUpperCase()}. ${s.title}: ${s.summary} ${SITE_URL}${startMdPath(s.id)}`,
    ),
    "",
    `Agents: bosun prints one JSON object per command (${fields}). Do what \`error.fix\` says; make the calls in \`next\`; never work around a refusal. Exit codes: ${codes}.`,
    "",
    "## Docs",
    "",
    `Only for detail: all in one file ${SITE_URL}/llms-full.txt; offline https://www.npmjs.com/package/@keelstackhq/docspack. Pages (Markdown):`,
    "",
    ...docLines,
    `- [Move a repo to bosun booster](${SITE_URL}/migrate.md)`,
    "",
    `Tools: ${toolLines}.`,
    "",
  ].join("\n");
}
