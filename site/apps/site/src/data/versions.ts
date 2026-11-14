// The one table of current tool versions. A bump is one edit here, then `node scripts/adopt.mjs`,
// the hero re-run (terminal in content.ts) and the Markdown pages that data/scenarios.test.ts names.
export const versions = {
  hull: "v0.10.0",
  splice: "0.4.0",
  bosun: "0.15.0",
  hoist: "0.4.1",
  hoard: "v0.5.0",
  berth: "0.1.0",
} as const;

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (): string =>
  `Last verified against bosun ${bare(versions.bosun)}, hull ${versions.hull}, splice ${versions.splice}.`;

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");
