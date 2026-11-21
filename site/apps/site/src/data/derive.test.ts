// derive(facts, legacy) is pure: the released state (the monorepo's lockstep pins and sources) is
// tested here with fixture facts, although the workspace version is not released yet (D-024).
import { describe, expect, test } from "vite-plus/test";
import { derive, type DeriveFacts, type Legacy } from "./derive.ts";
import { facts } from "./facts.ts";
import { legacy } from "./legacy.ts";

const sha = (c: string): string => c.repeat(40);

const fixtureLegacy: Legacy = {
  versions: {
    hull: "v0.9.0",
    splice: "0.4.0",
    bosun: "0.8.0",
    hoist: "0.3.0",
    hoard: "v0.2.0",
    berth: "0.1.0",
  },
  tagCommits: {
    hull: sha("1"),
    splice: sha("2"),
    bosun: sha("3"),
    hoist: sha("4"),
    hoard: sha("5"),
    berth: sha("6"),
  },
  verifiedOn: "2020-01-01",
  hullSource: "gh:old/hull/",
  hullRepo: "old/hull",
  decisionsChannel: "gh:old/decisions//channel",
  bosunChannel: "gh:old/bosun//channel",
  spliceActionUses: `old/splice/action@${sha("2")}`,
  spliceActionVersion: "0.4.0",
  hoardAction: "old/hoard",
  hoistSecretsAction: "old/hoist/secrets",
  checkedAgainst: { bosun: "0.7.0" },
};

const fixtureFacts = (released: boolean): DeriveFacts => ({
  released,
  version: "1.2.3",
  tag: "v1.2.3",
  repoSlug: "acme/mono",
  repoUrl: "https://github.com/acme/mono",
  decisionsChannel: "gh:acme/mono//decisions/channel",
  spliceAction: "acme/mono/splice/action",
  hullLegacyTags: { "v0.9.0": sha("a"), "v0.8.0": sha("b") },
});

describe("derive: released (the workspace version is a monorepo release)", () => {
  const d = derive(fixtureFacts(true), fixtureLegacy);

  test("every tool pins the workspace version (lockstep); hull and hoard as the tag", () => {
    expect(d.released).toBe(true);
    expect(d.versions).toEqual({
      hull: "v1.2.3",
      splice: "1.2.3",
      bosun: "1.2.3",
      hoist: "1.2.3",
      hoard: "v1.2.3",
      berth: "1.2.3",
    });
  });

  test("sources and actions name the monorepo", () => {
    expect(d.sources).toEqual({
      hull: "gh:acme/mono//hull",
      hullRepo: "acme/mono",
      hullDir: "hull/",
      decisions: "gh:acme/mono//decisions/channel",
      bosunChannel: "gh:acme/mono//bosun/channel",
      spliceActionUses: "acme/mono/splice/action@v1.2.3",
      spliceAction: "acme/mono/splice/action@v1.2.3 # v1.2.3",
      hoardAction: "acme/mono/hoard@v1.2.3",
      hoardMajor: "acme/mono/hoard@v1",
      hoistSecrets: "acme/mono/hoist/secrets",
    });
  });

  test("links pin the release tag", () => {
    expect(d.links).toEqual({
      repo: "https://github.com/acme/mono",
      main: "https://github.com/acme/mono/blob/main",
      tree: "https://github.com/acme/mono/tree/main",
      pinned: "https://github.com/acme/mono/blob/v1.2.3",
      pinnedLabel: "keelstack v1.2.3",
      hullRef: "v1.2.3",
      hull: "https://github.com/acme/mono/blob/v1.2.3/hull",
      hullShort: "v1.2.3",
    });
  });

  test("the real facts give the monorepo's own sources (gh:<org>/<repo>//hull/template, actions)", () => {
    const r = derive({ ...facts, released: true }, legacy);
    expect(`${r.sources.hull}/template`).toBe(facts.hullSource);
    expect(r.sources.decisions).toBe(facts.decisionsChannel);
    expect(r.sources.hoardAction).toBe(facts.hoardAction);
    expect(r.sources.spliceActionUses).toBe(`${facts.spliceAction}@${facts.tag}`);
    expect(r.sources.hoistSecrets).toBe(`${facts.repoSlug}/hoist/secrets`);
    for (const v of Object.values(r.versions)) expect(v.replace(/^v/, "")).toBe(facts.version);
  });
});

describe("derive: not released (the archived repos' last releases)", () => {
  const d = derive(fixtureFacts(false), fixtureLegacy);

  test("pins are legacy's", () => {
    expect(d.released).toBe(false);
    expect(d.versions).toEqual(fixtureLegacy.versions);
    expect(d.tagCommits).toEqual(fixtureLegacy.tagCommits);
  });

  test("sources and actions are the archived repos' (they still render)", () => {
    expect(d.sources).toEqual({
      hull: "gh:old/hull/",
      hullRepo: "old/hull",
      hullDir: "",
      decisions: "gh:old/decisions//channel",
      bosunChannel: "gh:old/bosun//channel",
      spliceActionUses: `old/splice/action@${sha("2")}`,
      spliceAction: `old/splice/action@${sha("2")} # v0.4.0`,
      hoardAction: "old/hoard@v0.2.0",
      hoardMajor: "old/hoard@v0",
      hoistSecrets: "old/hoist/secrets",
    });
  });

  test("links point at main and at hull's old tag commit in the monorepo", () => {
    expect(d.links).toEqual({
      repo: "https://github.com/acme/mono",
      main: "https://github.com/acme/mono/blob/main",
      tree: "https://github.com/acme/mono/tree/main",
      pinned: "https://github.com/acme/mono/blob/main",
      pinnedLabel: "the monorepo's `main`",
      hullRef: sha("a"),
      hull: `https://github.com/acme/mono/blob/${sha("a")}/hull`,
      hullShort: "aaaaaaa",
    });
  });

  test("throws when hull/legacy-tags.txt lacks the legacy hull tag", () => {
    const f = { ...fixtureFacts(false), hullLegacyTags: {} };
    expect(() => derive(f, fixtureLegacy)).toThrow(/legacy-tags\.txt has no v0\.9\.0/);
  });

  test("the released state does not need the legacy hull tag", () => {
    const f = { ...fixtureFacts(true), hullLegacyTags: {} };
    expect(derive(f, fixtureLegacy).links.hullRef).toBe("v1.2.3");
  });
});
