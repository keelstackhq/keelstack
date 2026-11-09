import { readyDocs } from "../data/docs";
import { agentRows, hero, quickstart, sections } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTION, DEFAULT_ON } from "./command";
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
