import { readyDocs } from "../data/docs";
import { agentRows, hero, sections } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTION, DEFAULT_ON } from "./command";
import { docMdPath } from "./docs-md";
import { SITE_URL } from "./site";
import docspack from "../../docspack.json";

export function buildLlmsTxt(): string {
  const on = new Set(DEFAULT_ON);
  const toolLines = tools.map((t) => `- [${t.name}](${t.repo}) (${t.status}): ${t.pitch}`);
  const agentLines = agentRows.map((r) => `- \`${r.key}\`: ${r.text}`);
  const docLines = readyDocs().map(
    (d) => `- [${d.title}](${SITE_URL}${docMdPath(d.slug)}): ${d.summary}`,
  );
  return [
    "# keelstack",
    "",
    `> ${hero.body}`,
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
    `All docs in one file: ${SITE_URL}/llms-full.txt. Offline with docspack: ${SITE_URL}/docspack/keelstackhq-docspack-${docspack.version}.tgz (see Getting started). Each page as Markdown:`,
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
