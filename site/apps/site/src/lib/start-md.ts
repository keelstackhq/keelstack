// /docs/start/<id>.md: Setup plus one scenario block, cut from docs/start.md so an agent reads
// only what it needs. The full page stays at /docs/start.md.
import { scenarios } from "../data/scenarios";
import { absoluteLinks, FRONTMATTER } from "./docs-md";
import { SITE_URL } from "./site";

const raw = Object.values(
  import.meta.glob("../pages/docs/start.md", { query: "?raw", import: "default", eager: true }),
)[0] as string;

const src = raw.replace(FRONTMATTER, "");
const section = (re: RegExp): string => (src.match(re)?.[0] ?? "").trim();

export const startMdPath = (id: string): string => `/docs/start/${id}.md`;

const fixed = (t: string): string =>
  absoluteLinks(t.replace("every block below assumes it", "every scenario assumes it"));

export function startScenarioMd(id: string): string {
  const s = scenarios.find((x) => x.id === id);
  if (!s) throw new Error(`no scenario ${id}`);
  const setup = section(/## Setup\n[\s\S]*?(?=\n1\. \[New repo\])/);
  const block = section(
    new RegExp(
      `<a id="${id}"></a>\\n\\n## ${id.toUpperCase()}\\. [\\s\\S]*?(?=\\n<a id="|\\n## For agents)`,
    ),
  )
    .replace(/<a id="[a-z]"><\/a>\n*/g, "")
    .replace(/^Needs \[Setup\]\(#setup\)\.\n*/m, "")
    .replace(/^## /, `# Start `)
    .replace(/\[Setup\]\(#setup\)/g, "Setup above")
    .replace(/\]\(#([a-f])\)/g, (_m, x: string) => `](${SITE_URL}${startMdPath(x)})`)
    .replace("](/migrate/)", `](${SITE_URL}/migrate.md)`);
  return [`> ${s.summary}`, "", fixed(setup), "", fixed(block), ""].join("\n");
}
