// The one table of current tool versions. A bump is one edit here, then `node scripts/adopt.mjs`,
// the hero re-run (terminal in content.ts) and the Markdown pages that data/scenarios.test.ts names.
export const versions = {
  hull: "v0.11.0",
  splice: "0.5.0",
  bosun: "0.15.0",
  hoist: "0.5.0",
  hoard: "v0.6.0",
  berth: "0.2.0",
} as const;

/** Commit each tag in `versions` points to (`git ls-remote`, peeled), checked 2026-11-15. */
export const tagCommits = {
  hull: "2e678a203c607779a50271fabcb9534ddc94ce06",
  splice: "85c95b63f53836301f2405368767c6ae9138109f",
  bosun: "bb4d1ab1b31903f47154a03e2197aa5fde630e0c",
  hoist: "85cb17573f8a835a577bce1fdaa653ffca5723b1",
  hoard: "aa00c826f05dff5230caa7c5f0e2ac0615ebf788",
  berth: "3711745cde10561aa754d0949a94f13b99428865",
} as const satisfies Record<keyof typeof versions, string>;

/** The date the versions above were last checked against their tags and docs. */
export const verifiedOn = "2026-11-15";

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (): string =>
  `Last verified ${verifiedOn} against bosun ${bare(versions.bosun)}, hull ${versions.hull}, splice ${versions.splice}.`;

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");
