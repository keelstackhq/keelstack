import { describe, expect, it } from "vite-plus/test";
import { findRoot } from "../data/facts";
import { launchersFor, resolvePublished, type FetchJson } from "./published";

// A fake registry: package -> version -> optionalDependencies.
type Reg = Record<string, Record<string, string[]>>;
const registry =
  (reg: Reg): FetchJson =>
  async (path) => {
    const seg = path.split("/");
    const scoped = path.startsWith("@");
    const name = scoped ? `${seg[0]}/${seg[1]}` : seg[0]!;
    const version = seg[scoped ? 2 : 1];
    const versions = reg[name];
    if (!versions) return undefined;
    if (!version)
      return { versions: Object.fromEntries(Object.keys(versions).map((v) => [v, {}])) };
    const deps = versions[version];
    return deps
      ? { optionalDependencies: Object.fromEntries(deps.map((d) => [d, version])) }
      : undefined;
  };

const L = ["@a/x", "@a/y"];
const full: Reg = {
  "@a/x": { "0.22.0": ["@a/x-linux"], "0.23.0": ["@a/x-linux"] },
  "@a/x-linux": { "0.22.0": [], "0.23.0": [] },
  "@a/y": { "0.23.0": ["@a/y-linux"] },
  "@a/y-linux": { "0.23.0": [] },
};

describe("resolvePublished", () => {
  it("returns the first candidate that is fully published", async () => {
    expect(await resolvePublished(["0.23.0", "0.22.0"], L, registry(full))).toBe("0.23.0");
  });
  it("does not require a tool added after the older candidate", async () => {
    expect(await resolvePublished(["0.22.0"], L, registry(full))).toBe("0.22.0");
  });
  it("falls back when a platform package is missing at the newer candidate", async () => {
    const reg = { ...full, "@a/y-linux": {} };
    expect(await resolvePublished(["0.23.0", "0.22.0"], L, registry(reg))).toBe("0.22.0");
  });
  it("falls back when a launcher is missing at the newer candidate", async () => {
    const reg = { ...full, "@a/y": { "0.22.5": [] } };
    expect(await resolvePublished(["0.23.0", "0.22.0"], L, registry(reg))).toBe("0.22.0");
  });
  it("is empty when nothing is published or there are no candidates", async () => {
    expect(await resolvePublished(["0.21.0"], L, registry(full))).toBe("");
    expect(await resolvePublished([], L, registry(full))).toBe("");
  });
  it("treats a rejecting or throwing registry as not published, and says why", async () => {
    const why: string[] = [];
    expect(
      await resolvePublished(
        ["0.22.0"],
        L,
        () => Promise.reject(new Error("net")),
        (m) => why.push(m),
      ),
    ).toBe("");
    expect(
      await resolvePublished(["0.22.0"], L, () => {
        throw new Error("sync");
      }),
    ).toBe("");
    expect(why.join()).toContain("net");
  });
  it("ignores candidates that are not X.Y.Z", async () => {
    expect(
      await resolvePublished(["v0.22.0", "0.22.0-rc.1", "junk", "0.23.0"], L, registry(full)),
    ).toBe("0.23.0");
  });
});

describe("launchersFor", () => {
  const list = launchersFor(findRoot());
  it("has the launchers, not platform or cargo-deny packages", () => {
    expect(list).toContain("@keelstackhq/bosun");
    expect(list).not.toContain("@keelstackhq/bosun-linux-x64");
    expect(list.some((p) => p.includes("cargo-deny"))).toBe(false);
  });
});
