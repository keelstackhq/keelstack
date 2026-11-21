// Renders docs-src templates: `{{ks.NAME}}` tokens become values from versions.ts (pins, sources
// and monorepo links, all derived from facts.ts) and the scenario builders.
// Imports only versions.ts, scenarios.ts and setup-script.ts (with .ts extensions) so Node can run it directly.
import { readSetupScript } from "./setup-script.ts";
import * as sc from "../data/scenarios.ts";
import {
  bare,
  checkedAgainst,
  links,
  MIN_BOSUN_MIGRATE,
  sources,
  verifiedLine,
  verifiedOn,
  versions,
  type Versions,
} from "../data/versions.ts";

export type Ctx = Record<string, string>;

export interface Refs {
  assessUrl?: string;
  assessSha?: string;
}

const fence = (body: string): string => "```bash\n" + body + "\n```";

export function buildCtx(
  v: Versions = versions,
  refs: Refs = {},
  verified: string = verifiedOn,
): Ctx {
  const ctx: Ctx = {};
  for (const t of Object.keys(v) as (keyof Versions)[]) {
    ctx[t] = v[t];
    ctx[`bare.${t}`] = bare(v[t]);
  }
  ctx["min.bosunMigrate"] = MIN_BOSUN_MIGRATE;
  ctx["checked.bosun"] = checkedAgainst.bosun;
  for (const [k, val] of Object.entries(sources)) ctx[`src.${k}`] = val;
  for (const [k, val] of Object.entries(links)) ctx[`link.${k}`] = val;
  for (const [k, val] of Object.entries(readSetupScript())) ctx[`script.${k}`] = val;
  ctx.verifiedOn = verified;
  ctx.verified = verifiedLine(v, verified);
  ctx["humanSteps.table"] = sc.humanStepsTable();
  ctx["humanSteps.list"] = sc.humanStepsList();
  ctx["humanSteps.alone"] = sc.bosunAloneHoist(v);
  ctx["humanSteps.note"] = sc.HUMAN_STEPS_NOTE;
  // One line per builder; add new ones here.
  const blocks: Record<string, string> = {
    installBlock: sc.installBlock(v),
    newRepoBlock: sc.newRepoBlock(v),
    hoistBlock: sc.hoistBlock(v),
    adoptBosunBlock: sc.adoptBosunBlock,
    upgradeBlock: sc.upgradeBlock(v),
    approveBlock: sc.approveBlock,
    migrateBlock: sc.MIGRATE_COMMANDS,
    channelsBlock: sc.channelsBlock(v),
    hoistAdoptBlock: sc.hoistAdoptBlock,
    adoptHullRunBlock: sc.adoptHullRunBlock(v),
    verifyBlock: sc.verifyBlock(v),
    rollbackBlock: sc.rollbackBlock,
  };
  if (refs.assessUrl && refs.assessSha) {
    ctx["assess.url"] = refs.assessUrl;
    ctx["assess.sha"] = refs.assessSha;
    blocks.adoptHullBlock = sc.adoptHullBlock(v, refs.assessUrl, refs.assessSha);
  }
  for (const [name, body] of Object.entries(blocks)) ctx[`block.${name}`] = fence(body);
  return ctx;
}

export function renderTemplate(src: string, ctx: Ctx, file: string): string {
  return src.replace(/\{\{ks\.([^}]*)\}\}/g, (token, name: string, offset: number) => {
    const value = ctx[name];
    if (value === undefined) {
      const line = src.slice(0, offset).split("\n").length;
      throw new Error(`gen-docs: unknown token ${token} in docs-src/${file}:${line}`);
    }
    return value;
  });
}
