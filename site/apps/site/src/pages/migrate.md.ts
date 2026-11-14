// /migrate.md: the same data as /migrate/, as Markdown for agents.
import { migrate, RUNBOOK_URL } from "../data/migrate";
import { docMdPath } from "../lib/docs-md";
import { SITE_URL } from "../lib/site";

const fence = (s: string) => `\`\`\`bash\n${s}\n\`\`\``;
const list = (xs: readonly string[], ordered = false) =>
  xs.map((x, i) => `${ordered ? `${i + 1}.` : "-"} ${x}`).join("\n");

export const GET = () => {
  const { quick, does, local, web, slack } = migrate;
  const md = [
    `# ${migrate.title}`,
    "",
    `> ${migrate.summary}`,
    "",
    migrate.forWho,
    "",
    migrate.owner,
    "",
    "## Quick start",
    "",
    quick.setupLabel,
    "",
    fence(quick.setup),
    "",
    quick.setupNote,
    "",
    quick.network,
    "",
    quick.promptLabel,
    "",
    fence(quick.prompt),
    "",
    quick.promptNote,
    "",
    quick.approve,
    "",
    "## What bosun migrate does",
    "",
    does.intro,
    "",
    fence(does.commands),
    "",
    list(does.notes),
    "",
    ...[local, web, slack].flatMap((s) => [`## ${s.heading}`, "", list(s.steps, true), ""]),
    "## The mode ladder",
    "",
    ...migrate.ladder.map((r) => `- ${r[0]}: ${r[1]} Owner touches: ${r[2]}.`),
    "",
    fence(migrate.flags),
    "",
    list(migrate.notes),
    "",
    migrate.fastLane,
    "",
    `Full guide: ${RUNBOOK_URL}`,
    "",
  ]
    .join("\n")
    .replace(/\/docs\/([a-z-]+)\//g, (_m, slug: string) => `${SITE_URL}${docMdPath(slug)}`);
  return new Response(md, { headers: { "Content-Type": "text/markdown; charset=utf-8" } });
};
