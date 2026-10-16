import { agentRows, hero, sections } from "../data/content";
import { tools } from "../data/tools";
import { buildCommand, CAPTIONS, DEFAULT_ON } from "./command";

export function buildLlmsTxt(): string {
  const on = new Set(DEFAULT_ON);
  const toolLines = tools.map((t) => `- [${t.name}](${t.repo}) (${t.status}): ${t.pitch}`);
  const agentLines = agentRows.map((r) => `- \`${r.key}\`: ${r.text}`);
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
    "## Get started",
    "",
    sections.start.body ?? "",
    "",
    "Today:",
    "",
    "```sh",
    buildCommand(on, "today"),
    "```",
    "",
    "With splice 0.1:",
    "",
    CAPTIONS.splice,
    "",
    "```sh",
    buildCommand(on, "splice"),
    "```",
    "",
  ].join("\n");
}
