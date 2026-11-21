// History, frozen: the last releases of the archived per-tool repos (D-023), which the /docs pages
// were last verified against. Those repos are archived, but bosun released 0.21.0 from its old repo
// after the import, so this table did go stale once: re-check it against npm and the old repos' tags
// (`git ls-remote --tags`) until the first monorepo release. It is used only while the monorepo has no
// release (`facts.released` is false): their npm packages, tags and `gh:` sources really resolve,
// while the workspace version (and its tag) is not published. derive.ts switches every pin to facts.ts
// at the first monorepo release. The only file allowed to type a version or an archived repo
// (facts.test.ts scans versions.ts, derive.ts, scenarios.ts, docs-src and the build).
import type { Legacy } from "./derive.ts";

export const legacy = {
  versions: {
    hull: "v0.13.0",
    splice: "0.6.0",
    bosun: "0.21.0",
    hoist: "0.5.1",
    hoard: "v0.6.1",
    berth: "0.2.1",
  },
  /** Commit each tag in `versions` points to in its archived repo (`git ls-remote`, peeled): 2026-11-16, bosun 2026-11-21. */
  tagCommits: {
    hull: "3f4a65fda568cdd20c76168ed156d25a28ba5630",
    splice: "feea8496215afce057f98eaea0e303aced93346c",
    bosun: "18ab5b2ab388dfdf18251c43e9208d9c7fad8784",
    hoist: "5f21d0d0732651999d3e33912c1a287c385ecb9b",
    hoard: "4a6d185fb73af3d973eaa256a88bc0d02c68b63a",
    berth: "cab67fa16a1107df8219a26581c7300a19a71f76",
  },
  /** The date the versions above were last checked against npm `latest` and their repos' tags. */
  verifiedOn: "2026-11-21",
  /** Sources that resolve at those releases (D-023: the archived repos' sources still render). */
  hullSource: "gh:keelstackhq/hull/",
  hullRepo: "keelstackhq/hull",
  decisionsChannel: "gh:keelstackhq/decisions//channel",
  bosunChannel: "gh:keelstackhq/bosun//channel",
  /** The splice action pin bosun 0.20.0's and 0.21.0's channels seed (splice.yml). */
  spliceActionUses: "keelstackhq/splice/action@feea8496215afce057f98eaea0e303aced93346c",
  spliceActionVersion: "0.6.0",
  hoardAction: "keelstackhq/hoard",
  /** hoist's secrets action in its archived repo (hull v0.13.0's `secrets` piece uses it). */
  hoistSecretsAction: "keelstackhq/hoist/secrets",
  /** The bosun release the ci, risk, tasks, operating-model and greenfield pages were checked against line by line. */
  checkedAgainst: { bosun: "0.19.0" },
} as const satisfies Legacy;
