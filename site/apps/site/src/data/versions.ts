// The pins every /docs page, /llms.txt, /agents.md and the scenario blocks show: derive(facts,
// legacy) from derive.ts (D-024; the rule and both states are described and tested there),
// applied to this repository. After a version bump, run the gen script.
import { facts } from "./facts.ts";
import { legacy } from "./legacy.ts";
import { bare, derive, type Tool, type Versions } from "./derive.ts";
import { readSetupScript } from "../lib/setup-script.ts";

export { bare, type Tool, type Versions };

const d = derive(facts, legacy);

/** True once the workspace version is a monorepo release. */
export const released: boolean = d.released;

/** Current pins: hull and hoard as tags (`vX.Y.Z`), the npm tools as bare versions. */
export const versions: Versions = d.versions;

/** The archived repos' tag commits (history: the copies in data/hull were fetched at these). */
export const tagCommits: Versions = d.tagCommits;

/** Sources and actions the commands use. */
export const sources = d.sources;

const script = readSetupScript();

/** Links into the monorepo. */
export const links = {
  ...d.links,
  /** bosun's setup script copy (data/bosun/setup-script.json) at its commit here. */
  scriptRef: script.commit,
  script: `${facts.repoUrl}/blob/${script.commit}/bosun/docs/setup-script.sh`,
} as const;

/** The bosun release some pages were last checked line by line against (history, legacy.ts). */
export const checkedAgainst: { bosun: string } = { ...legacy.checkedAgainst };

/** The pins in one sentence (/llms.txt), saying which rule applies. */
export const pinsLine = (v: Versions = versions, isReleased: boolean = released): string => {
  const list = `hull ${v.hull}, splice ${v.splice}, bosun ${v.bosun}, hoist ${v.hoist}, hoard ${v.hoard}, berth ${v.berth}`;
  return isReleased
    ? `Current versions: ${list} (keelstack ${facts.tag}, one version for every tool).`
    : `Current versions: ${list} (the old tool repos' last releases; the monorepo has no release yet).`;
};

/** Oldest bosun with `bosun migrate --to <mode>` and `--finish` (bosun CHANGELOG). */
export const MIN_BOSUN_MIGRATE = "0.15.0";

/** The date the pins were last checked: the shown release's date (bosun/CHANGELOG.md) once the
 * page follows a monorepo release, else the archived releases' check (legacy.ts). */
export const verifiedOn: string =
  facts.released && facts.versionDate ? facts.versionDate : legacy.verifiedOn;

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (
  v: Versions = versions,
  on: string = verifiedOn,
  isReleased: boolean = released,
): string =>
  isReleased
    ? `Every pin is keelstack ${facts.tag}, one version for every tool.`
    : `Last verified ${on} against bosun ${bare(v.bosun)}, hull ${v.hull}, splice ${v.splice} (the old tool repos' last releases, pinned until keelstack's first release).`;
