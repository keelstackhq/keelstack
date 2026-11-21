// The pins, sources and links the docs show, as a pure function of facts.ts and legacy.ts (D-024).
// The rule: a command shows only what resolves today.
// - Once the monorepo has released its workspace version (`facts.released`), every tool pins that
//   version (lockstep), hull and hoard its tag, and sources and actions name the monorepo
//   (`gh:<org>/<repo>//hull/template`, `<org>/<repo>/<tool>@vX`).
// - Before that, commands pin the archived repos' last releases and their sources (legacy.ts),
//   which still resolve (D-023).
// Links to files always point at the monorepo: `main` (or the release tag), hull's old release at
// its commit in this repository (hull/legacy-tags.txt).
// Type-only imports with .ts extensions, so Node can run the docs generator directly.
import type { Facts } from "./facts.ts";

export type Tool = "hull" | "splice" | "bosun" | "hoist" | "hoard" | "berth";
export type Versions = Record<Tool, string>;

/** The facts derive() reads. */
export type DeriveFacts = Pick<
  Facts,
  | "released"
  | "version"
  | "tag"
  | "repoSlug"
  | "repoUrl"
  | "decisionsChannel"
  | "spliceAction"
  | "hullLegacyTags"
>;

/** Frozen history: the archived repos' last releases (the shape of legacy.ts). */
export interface Legacy {
  versions: Versions;
  /** Commit each tag in `versions` points to in its archived repo. */
  tagCommits: Versions;
  verifiedOn: string;
  hullSource: string;
  hullRepo: string;
  decisionsChannel: string;
  bosunChannel: string;
  spliceActionUses: string;
  spliceActionVersion: string;
  hoardAction: string;
  hoistSecretsAction: string;
  /** The tool versions some pages were last checked line by line against (history). */
  checkedAgainst: { bosun: string };
}

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");

const major = (v: string): string => `v${bare(v).split(".")[0]!}`;

export function derive(f: DeriveFacts, l: Legacy) {
  const released = f.released;
  const versions: Versions = released
    ? {
        hull: f.tag,
        splice: f.version,
        bosun: f.version,
        hoist: f.version,
        hoard: f.tag,
        berth: f.version,
      }
    : { ...l.versions };

  const hoardBase = released ? `${f.repoSlug}/hoard` : l.hoardAction;
  const spliceUses = released ? `${f.spliceAction}@${f.tag}` : l.spliceActionUses;
  const sources = {
    /** Append `/template`, `/template-rust` or `/rigging`. */
    hull: released ? `gh:${f.repoSlug}//hull` : l.hullSource,
    /** The GitHub repository of the hull source, and hull's directory in it. */
    hullRepo: released ? f.repoSlug : l.hullRepo,
    hullDir: released ? "hull/" : "",
    decisions: released ? f.decisionsChannel : l.decisionsChannel,
    bosunChannel: released ? `gh:${f.repoSlug}//bosun/channel` : l.bosunChannel,
    spliceActionUses: spliceUses,
    /** `uses:` value with its version comment. */
    spliceAction: `${spliceUses} # v${released ? f.version : l.spliceActionVersion}`,
    hoardAction: `${hoardBase}@${versions.hoard}`,
    hoardMajor: `${hoardBase}@${major(versions.hoard)}`,
    /** hoist's secrets action (append `@<ref>`). */
    hoistSecrets: released ? `${f.repoSlug}/hoist/secrets` : l.hoistSecretsAction,
  };

  const hullRef = released ? f.tag : f.hullLegacyTags[l.versions.hull];
  if (!hullRef) throw new Error(`versions: hull/legacy-tags.txt has no ${l.versions.hull}`);
  const pinRef = released ? f.tag : "main";
  const links = {
    repo: f.repoUrl,
    /** `<repo>/blob/main`; append `/<folder>/<path>`. */
    main: `${f.repoUrl}/blob/main`,
    /** `<repo>/tree/main`; append `/<folder>`. */
    tree: `${f.repoUrl}/tree/main`,
    /** Specs the pages follow: the release tag, else `main`. */
    pinned: `${f.repoUrl}/blob/${pinRef}`,
    pinnedLabel: released ? `keelstack ${f.tag}` : "the monorepo's `main`",
    /** hull at the pinned release: its tag, else the old tag's commit in this repository. */
    hullRef,
    hull: `${f.repoUrl}/blob/${hullRef}/hull`,
    hullShort: released ? hullRef : hullRef.slice(0, 7),
  };

  return { released, versions, tagCommits: { ...l.tagCommits }, sources, links };
}
