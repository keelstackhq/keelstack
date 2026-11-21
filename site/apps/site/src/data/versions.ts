// The pins every /docs page, /llms.txt, /agents.md and the scenario blocks show, derived from
// facts.ts (D-024). The rule: a command shows only what resolves today.
// - Once the monorepo has released its workspace version (`facts.released`: the root Cargo.toml
//   version is above the move version in hull/legacy-tags.txt), every tool pins that version, hull
//   its tag, and sources and actions name the monorepo (`gh:<org>/<repo>//hull/template`,
//   `<org>/<repo>/<tool>@vX`).
// - Before that, the workspace version is not on npm and has no tag, so commands pin the archived
//   repos' last releases (legacy.ts, frozen history) and their sources, which still render (D-023).
// Links to files always point at the monorepo: `main` (or the release tag), hull's release at its
// commit in this repository (hull/legacy-tags.txt). After a version bump, run the gen script.
import { facts } from "./facts.ts";
import { legacy } from "./legacy.ts";
import { readSetupScript } from "../lib/setup-script.ts";

export type Tool = keyof typeof legacy.versions;
export type Versions = Record<Tool, string>;

/** True once the workspace version is a monorepo release. */
export const released: boolean = facts.released;

const lockstep: Versions = {
  hull: facts.tag,
  splice: facts.version,
  bosun: facts.version,
  hoist: facts.version,
  hoard: facts.tag,
  berth: facts.version,
};

/** Current pins: hull and hoard as tags (`vX.Y.Z`), the npm tools as bare versions. */
export const versions: Versions = released ? lockstep : { ...legacy.versions };

/** The archived repos' tag commits (history: the copies in data/hull were fetched at these). */
export const tagCommits: Versions = { ...legacy.tagCommits };

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");

const major = (v: string): string => `v${bare(v).split(".")[0]!}`;

const hoardBase = released ? `${facts.repoSlug}/hoard` : legacy.hoardAction;
const spliceUses = released ? `${facts.spliceAction}@${facts.tag}` : legacy.spliceActionUses;

/** Sources and actions the commands use (see the rule above). */
export const sources = {
  /** Append `/template`, `/template-rust` or `/rigging`. */
  hull: released ? `gh:${facts.repoSlug}//hull` : legacy.hullSource,
  /** The GitHub repository of the hull source, and hull's directory in it. */
  hullRepo: released ? facts.repoSlug : legacy.hullRepo,
  hullDir: released ? "hull/" : "",
  decisions: released ? facts.decisionsChannel : legacy.decisionsChannel,
  bosunChannel: released ? `gh:${facts.repoSlug}//bosun/channel` : legacy.bosunChannel,
  spliceActionUses: spliceUses,
  /** `uses:` value with its version comment. */
  spliceAction: `${spliceUses} # v${released ? facts.version : legacy.spliceActionVersion}`,
  hoardAction: `${hoardBase}@${versions.hoard}`,
  hoardMajor: `${hoardBase}@${major(versions.hoard)}`,
} as const;

const hullRef = released ? facts.tag : facts.hullLegacyTags[legacy.versions.hull];
if (!hullRef) throw new Error(`versions: hull/legacy-tags.txt has no ${legacy.versions.hull}`);
const pinRef = released ? facts.tag : "main";
const script = readSetupScript();

/** Links into the monorepo. */
export const links = {
  repo: facts.repoUrl,
  /** `<repo>/blob/main`; append `/<folder>/<path>`. */
  main: `${facts.repoUrl}/blob/main`,
  /** `<repo>/tree/main`; append `/<folder>`. */
  tree: `${facts.repoUrl}/tree/main`,
  /** Specs the pages follow: the release tag, else `main`. */
  pinned: `${facts.repoUrl}/blob/${pinRef}`,
  pinnedLabel: released ? `keelstack ${facts.tag}` : "the monorepo's `main`",
  /** hull at the pinned release: its tag, else the old tag's commit in this repository. */
  hullRef,
  hull: `${facts.repoUrl}/blob/${hullRef}/hull`,
  hullShort: released ? hullRef : hullRef.slice(0, 7),
  /** bosun's setup script copy (data/bosun/setup-script.json) at its commit here. */
  scriptRef: script.commit,
  script: `${facts.repoUrl}/blob/${script.commit}/bosun/docs/setup-script.sh`,
} as const;

/** The pins in one sentence (/llms.txt), saying which rule applies. */
export const pinsLine = (v: Versions = versions, isReleased: boolean = released): string => {
  const list = `hull ${v.hull}, splice ${v.splice}, bosun ${v.bosun}, hoist ${v.hoist}, hoard ${v.hoard}, berth ${v.berth}`;
  return isReleased
    ? `Current versions: ${list} (keelstack ${facts.tag}, one version for every tool).`
    : `Current versions: ${list} (the old tool repos' last releases; the monorepo has no release yet).`;
};

/** Oldest bosun with `bosun migrate --to <mode>` and `--finish` (bosun CHANGELOG). */
export const MIN_BOSUN_MIGRATE = "0.15.0";

/** The date the pins were last checked (the archived releases; history in legacy.ts). */
export const verifiedOn: string = legacy.verifiedOn;

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (
  v: Versions = versions,
  on: string = verifiedOn,
  isReleased: boolean = released,
): string =>
  isReleased
    ? `Every pin is keelstack ${facts.tag}, one version for every tool.`
    : `Last verified ${on} against bosun ${bare(v.bosun)}, hull ${v.hull}, splice ${v.splice} (the old tool repos' last releases, pinned until keelstack's first release).`;
