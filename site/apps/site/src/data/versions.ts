// The one table of current tool versions. A bump is one edit here, then `node scripts/adopt.mjs`,
// the hero re-run (terminal in content.ts) and the Markdown pages that data/scenarios.test.ts names.
export const versions = {
  hull: "v0.12.0",
  splice: "0.5.1",
  bosun: "0.17.0",
  hoist: "0.5.1",
  hoard: "v0.6.0",
  berth: "0.2.1",
} as const;

/** Commit each tag in `versions` points to (`git ls-remote`, peeled), checked 2026-11-15. */
export const tagCommits = {
  hull: "5a72442649f6958e91a50283a91e985f45e10a09",
  splice: "8dcdf9a9faf35cd5b504c326c9107f8da689264e",
  bosun: "0d2bcd700251773b0191af8a31062f6401c1edb1",
  hoist: "5f21d0d0732651999d3e33912c1a287c385ecb9b",
  hoard: "aa00c826f05dff5230caa7c5f0e2ac0615ebf788",
  berth: "cab67fa16a1107df8219a26581c7300a19a71f76",
} as const satisfies Record<keyof typeof versions, string>;

/** The date the versions above were last checked against their tags and docs. */
export const verifiedOn = "2026-11-15";

/** The one "last verified" line: shown once per page by components/Verified.astro and docs-md.ts. */
export const verifiedLine = (): string =>
  `Last verified ${verifiedOn} against bosun ${bare(versions.bosun)}, hull ${versions.hull}, splice ${versions.splice}.`;

/** `v0.10.0` -> `0.10.0` */
export const bare = (v: string): string => v.replace(/^v/, "");
