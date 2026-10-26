import { docHref, readyDocs } from "../data/docs";
import { agentRows, hero, sections } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTION, DEFAULT_ON } from "./command";
import { SITE_URL } from "./site";

export function buildLlmsTxt(): string {
  const on = new Set(DEFAULT_ON);
  const toolLines = tools.map((t) => `- [${t.name}](${t.repo}) (${t.status}): ${t.pitch}`);
  const agentLines = agentRows.map((r) => `- \`${r.key}\`: ${r.text}`);
  const docLines = readyDocs().map(
    (d) => `- [${d.title}](${SITE_URL}${docHref(d.slug)}): ${d.summary}`,
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
