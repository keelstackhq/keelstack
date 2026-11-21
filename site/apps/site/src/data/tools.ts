// Tool cards. Every fact (version, description, subcommands, links) comes from data/facts.ts, which
// reads the monorepo sources (D-024). Only the editorial labels below are written by hand; a test
// fails when a tool has no label or a label has no tool.
import { facts } from "./facts";
import { sources } from "./versions";

export const org = { name: "keelstackhq", url: facts.orgUrl } as const;

/** Editorial: card order, the number-and-role label, and the one-word verb of the heading. */
export const roles: Record<string, { role: string; verb: string }> = {
  hull: { role: "monorepo template", verb: "Template" },
  splice: { role: "bootstrap + update", verb: "Bootstrap" },
  hoard: { role: "cached CI", verb: "Cache" },
  bosun: { role: "agent orchestration", verb: "Orchestrate" },
  hoist: { role: "provision + release", verb: "Provision" },
  berth: { role: "agent workspaces", verb: "Isolate" },
  keel: { role: "guided setup", verb: "Guide" },
};

export interface Tool {
  name: string;
  role: string;
  verb: string;
  status: string;
  statusStyle: "filled" | "outline" | "dashed";
  pitch: string;
  detail: string;
  detailMono: boolean;
  repo: string;
}

const SHOWN = 8;

function detail(name: string): { text: string; mono: boolean } {
  const f = facts.tools.find((t) => t.name === name)!;
  if (name === "hull") {
    const ids = facts.pieces.filter((p) => p.template === "template").map((p) => p.id);
    return { text: `pieces: ${ids.join(", ")}`, mono: false };
  }
  const names = f.subcommands.map((s) => s.name);
  const shown = names.slice(0, SHOWN).join(" · ");
  const more = names.length > SHOWN ? ` · … (${names.length} commands)` : "";
  const action = name === "hoard" ? `uses: ${sources.hoardAction} · ` : "";
  return { text: `${action}${shown}${more}`, mono: true };
}

export const tools: Tool[] = Object.keys(roles).map((name, i) => {
  const f = facts.tools.find((t) => t.name === name);
  if (!f) throw new Error(`tools: ${name} has a label but no folder in the monorepo`);
  const d = detail(name);
  return {
    name,
    role: `${String(i + 1).padStart(2, "0")} · ${roles[name]!.role}`,
    verb: roles[name]!.verb,
    status: facts.tag,
    statusStyle: i === 0 ? "filled" : i === 2 ? "dashed" : "outline",
    pitch: f.description,
    detail: d.text,
    detailMono: d.mono,
    repo: f.url,
  };
});
