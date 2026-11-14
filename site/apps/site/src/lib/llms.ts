import { readyDocs } from "../data/docs";
import { hero, quickstart } from "../data/content";
import { tools } from "../data/tools";
import { RUNBOOK_URL } from "../data/migrate";
import { scenarios } from "../data/scenarios";
import { versions } from "../data/versions";
import { docMdPath } from "./docs-md";
import { SITE_URL } from "./site";

// An index, not a copy: every exact command block lives once, in /docs/start.md.
export function buildLlmsTxt(): string {
  const toolLines = tools.map((t) => `- [${t.name}](${t.repo}) (${t.status}): ${t.pitch}`);
  const docLines = readyDocs().map(
    (d) => `- [${d.title}](${SITE_URL}${docMdPath(d.slug)}): ${d.summary}`,
  );
  const fields = quickstart.agents.fields.map((f) => `\`${f}\``).join(", ");
  const codes = quickstart.agents.exitCodes.map(([n, name]) => `${n} ${name}`).join(", ");
  return [
    "# keelstack",
    "",
    `> ${hero.body}`,
    "",
    "## Start here",
    "",
    `Current versions: hull ${versions.hull}, splice ${versions.splice}, bosun ${versions.bosun}, hoist ${versions.hoist}, hoard ${versions.hoard}, berth ${versions.berth}. Read ${SITE_URL}${docMdPath("start")}: a one-time setup block, then one exact copy-paste block per scenario.`,
    "",
    ...scenarios.map(
      (s) => `- ${s.id.toUpperCase()}. ${s.title}: ${s.summary} ${SITE_URL}/docs/start/#${s.id}`,
    ),
    "",
    `Agents: bosun prints one JSON object per command (${fields}). Do what \`error.fix\` says; make the calls in \`next\`; never work around a refusal. Exit codes: ${codes}.`,
    "",
    `Upgrade: ${SITE_URL}${docMdPath("update")} (hull: \`splice update --all\`; bosun: \`bosun pin --latest --pr\`). Move a repo to bosun booster: ${SITE_URL}/migrate/ (${RUNBOOK_URL}).`,
    "",
    "## Docs",
    "",
    `All docs in one file: ${SITE_URL}/llms-full.txt. Offline: https://www.npmjs.com/package/@keelstackhq/docspack. Each page as Markdown:`,
    "",
    ...docLines,
    "",
    "## Tools",
    "",
    ...toolLines,
    "",
  ].join("\n");
}
