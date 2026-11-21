// The one table of current tool versions. A bump is one edit here, then `node scripts/adopt.mjs`,
// the hero re-run (terminal in content.ts) and the Markdown pages that data/scenarios.test.ts names.
export const versions = {
  hull: "v0.13.0",
  splice: "0.6.0",
  bosun: "0.20.0",
  hoist: "0.5.1",
  hoard: "v0.6.1",
  berth: "0.2.1",
} as const;

/** Commit each tag in `versions` points to (`git ls-remote`, peeled), checked 2026-11-16. */
export const tagCommits = {
  hull: "3f4a65fda568cdd20c76168ed156d25a28ba5630",
  splice: "feea8496215afce057f98eaea0e303aced93346c",
  bosun: "aef4c80213e72d90ce4313dff234ac52701cd5d8",
  hoist: "5f21d0d0732651999d3e33912c1a287c385ecb9b",
  hoard: "4a6d185fb73af3d973eaa256a88bc0d02c68b63a",
  berth: "cab67fa16a1107df8219a26581c7300a19a71f76",
} as const satisfies Record<keyof typeof versions, string>;

/**
 * Fixed refs the docs templates cite (`{{ks.ref.*}}`): bosun's spec at its 0.20.0 tag; the splice action
 * pin is what bosun v0.20.0's channel seeds (splice.yml); bump when a bosun release seeds a newer action.
 */
export const docRefs = {
  bosunSpec: tagCommits.bosun,
  spliceAction: "feea8496215afce057f98eaea0e303aced93346c",
  spliceActionVersion: "0.6.0",
} as const;

/** Oldest bosun with `bosun migrate --to <mode>` and `--finish` (first shipped in 0.15.0, bosun CHANGELOG "## 0.15.0"). */
export const MIN_BOSUN_MIGRATE = "0.15.0";

/** The date the versions above were last checked against their tags and docs. */
export const verifiedOn = "2026-11-18";

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (v: typeof versions = versions, on: string = verifiedOn): string =>
  `Last verified ${on} against bosun ${bare(v.bosun)}, hull ${v.hull}, splice ${v.splice}.`;

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");
