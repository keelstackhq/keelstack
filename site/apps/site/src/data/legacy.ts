// History, frozen: the last releases of the archived per-tool repos (D-023), which the /docs pages
// were last verified against. Those repos never release again, so this table cannot go stale. It is
// used only while the monorepo has no release (`facts.released` is false): their npm packages, tags
// and `gh:` sources really resolve, while the workspace version (and its tag) is not published.
// The first monorepo release switches every pin to facts.ts (versions.ts). The only file allowed
// to type a version or an archived repo (facts.test.ts scans versions.ts, scenarios.ts, docs-src).
export const legacy = {
  versions: {
    hull: "v0.13.0",
    splice: "0.6.0",
    bosun: "0.20.0",
    hoist: "0.5.1",
    hoard: "v0.6.1",
    berth: "0.2.1",
  },
  /** Commit each tag in `versions` points to in its archived repo (`git ls-remote`, peeled), 2026-11-16. */
  tagCommits: {
    hull: "3f4a65fda568cdd20c76168ed156d25a28ba5630",
    splice: "feea8496215afce057f98eaea0e303aced93346c",
    bosun: "aef4c80213e72d90ce4313dff234ac52701cd5d8",
    hoist: "5f21d0d0732651999d3e33912c1a287c385ecb9b",
    hoard: "4a6d185fb73af3d973eaa256a88bc0d02c68b63a",
    berth: "cab67fa16a1107df8219a26581c7300a19a71f76",
  },
  /** The date the versions above were last checked against their tags and docs. */
  verifiedOn: "2026-11-18",
  /** Sources that resolve at those releases (D-023: the archived repos' sources still render). */
  hullSource: "gh:keelstackhq/hull/",
  hullRepo: "keelstackhq/hull",
  decisionsChannel: "gh:keelstackhq/decisions//channel",
  bosunChannel: "gh:keelstackhq/bosun//channel",
  /** The splice action pin bosun 0.20.0's channel seeds (splice.yml). */
  spliceActionUses: "keelstackhq/splice/action@feea8496215afce057f98eaea0e303aced93346c",
  spliceActionVersion: "0.6.0",
  hoardAction: "keelstackhq/hoard",
} as const;
