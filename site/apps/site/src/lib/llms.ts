import { readyDocs } from "../data/docs";
import { agentRows, hero, quickstart, sections, update } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTION, DEFAULT_ON } from "./command";
import { migrate, RUNBOOK_URL } from "../data/migrate";
import { docMdPath } from "./docs-md";
import { SITE_URL } from "./site";

export function buildLlmsTxt(): string {
  const on = new Set(DEFAULT_ON);
  const toolLines = tools.map((t) => `- [${t.name}](${t.repo}) (${t.status}): ${t.pitch}`);
  const agentLines = agentRows.map((r) => `- \`${r.key}\`: ${r.text}`);
  const docLines = readyDocs().map(
    (d) => `- [${d.title}](${SITE_URL}${docMdPath(d.slug)}): ${d.summary}`,
  );
  const steps = quickstart.steps.map(
    (s, i) => `${i + 1}. ${s.cmd ? `\`${s.cmd}\`: ` : ""}${s.text}`,
  );
  const fields = quickstart.agents.fields.map((f) => `\`${f}\``).join(", ");
  const codes = quickstart.agents.exitCodes.map(([n, name]) => `${n} ${name}`).join(", ");
  return [
    "# keelstack",
    "",
    `> ${hero.body}`,
    "",
    "## Start in 5 minutes",
    "",
    `Humans, with bosun: ${quickstart.body} Full page: ${SITE_URL}${docMdPath("start")}`,
    "",
    ...steps,
    "",
    `Agents: call bosun commands and act on the one JSON object each prints (${fields}). Do what \`error.fix\` says; make the calls in \`next\`. Exit codes: ${codes}.`,
    "",
    "## Update",
    "",
    `Page: ${SITE_URL}${docMdPath("update")}`,
    "",
    ...[update.hull, update.bosun].flatMap((p) => [
      `### ${p.heading}`,
      "",
      ...p.steps.map(
        (s, i) =>
          `${i + 1}. ${s.cmd ? `\`${s.cmd}\`` : (s.text ?? "")}${s.cmd && s.note ? ` # ${s.note}` : ""}`,
      ),
      "",
      ...p.notes.map((n) => `- ${n}`),
      `- Full guide: ${p.guide}`,
      "",
    ]),
    "## Tools",
    "",
    ...toolLines,
    "",
    "## For agents",
    "",
    sections.agents.body ?? "",
    "",
    ...agentLines,
    "",
    "## Docs",
    "",
    `All docs in one file: ${SITE_URL}/llms-full.txt. Offline with docspack: https://www.npmjs.com/package/@keelstackhq/docspack (see Getting started). Each page as Markdown:`,
    "",
    ...docLines,
    "",
    "## Move a repo to bosun booster",
    "",
    `${migrate.forWho} Page: ${SITE_URL}/migrate/`,
    "",
    `Sections: Claude Code locally, Claude Code on the web (setup script, network hosts, variables vs secrets, [cloud] environment_id), Claude Tag in Slack; each ends with a prompt for Claude. Full runbook: ${RUNBOOK_URL}`,
    "",
    "## Get started",
    "",
    sections.start.body ?? "",
    "",
    CAPTION,
    "",
    "```sh",
    buildCommand(on),
    "```",
    "",
  ].join("\n");
}
