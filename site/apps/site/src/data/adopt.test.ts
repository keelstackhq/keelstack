// "Adopt an existing repo" (/docs/adopt/) is hull's docs/adopt.md, copied verbatim at a pinned hull
// ref, and keelstack.dev hosts hull's ready.mjs (the assessment) at /assess/<tag>.mjs from the
// same ref. Older tags stay hosted, since old links pin them. Re-copy with apps/site/scripts/adopt.mjs.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docs } from "./docs";
import { facts } from "./facts";
import { versions } from "./versions";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const sha = (s: string) => createHash("sha256").update(s).digest("hex");
const source = JSON.parse(read("./hull/adopt.json")) as {
  repo: string;
  ref: string;
  doc: { path: string; sha256: string };
  assess: { path: string; url: string; sha256: string };
  hosted: Record<string, { ref: string; sha256: string }>;
};
const copy = read("./hull/adopt.md");
const hosted = (tag: string) => read(`./hull/assess/${tag}.mjs.txt`);
const page = read("../pages/docs/adopt.md");

describe("Adopt an existing repo", () => {
  it("pins hull's adopt.md and ready.mjs to one commit", () => {
    // Since the first monorepo release the copies come from hull/ in this repository.
    expect(source.repo).toBe("keelstackhq/keelstack");
    expect(source.ref).toMatch(/^[0-9a-f]{40}$/);
    expect(source.doc.path).toBe("hull/docs/adopt.md");
    expect(sha(copy)).toBe(source.doc.sha256);
    expect(source.assess.path).toBe("hull/template/pieces/base/files/scripts/ready.mjs");
  });

  it("hosts the copied assessment at the workspace version's URL, and the shown hull's too", () => {
    // The copy is of this commit's hull (the workspace version); the page may show an older hull.
    const own = `v${facts.workspaceVersion}`;
    expect(source.assess.url).toBe(`/assess/${own}.mjs`);
    expect(source.hosted[own]).toEqual({ ref: source.ref, sha256: source.assess.sha256 });
    expect(sha(hosted(own))).toBe(source.assess.sha256);
    expect(Object.keys(source.hosted)).toContain(versions.hull);
    expect(sha(hosted(versions.hull))).toBe(source.hosted[versions.hull]!.sha256);
  });

  it("keeps every earlier assessment served byte for byte", () => {
    // Releases only add to the list (scripts/release-pr.sh adds the new one): the tags below
    // must stay hosted, and the directory must hold exactly the tags adopt.json records.
    const kept = [
      "v0.10.0",
      "v0.11.0",
      "v0.12.0",
      "v0.12.1",
      "v0.13.0",
      "v0.22.0",
      "v0.22.1",
      "v0.6.0",
      "v0.6.1",
      "v0.6.2",
      "v0.7.0",
      "v0.8.0",
      "v0.9.0",
    ];
    expect(Object.keys(source.hosted)).toEqual(expect.arrayContaining(kept));
    const files = readdirSync(new URL("./hull/assess/", import.meta.url));
    expect(files.map((f) => f.replace(/\.mjs\.txt$/, "")).toSorted()).toEqual(
      Object.keys(source.hosted).toSorted(),
    );
    expect(source.hosted["v0.6.0"]).toEqual({
      ref: "9b4a0e44e10470d5aa80cc07052440cfa92a7496",
      sha256: "c425300387529849b84c04bcb3fb6a38aba4de920c84fdf7fe792e7e2fb4691d",
    });
    expect(source.hosted["v0.6.1"]).toEqual({
      ref: "031bdc1391aa75fabc01a5e60bfad980862e35bc",
      sha256: "77f7f697a6b066444215b529d0aba753bffd43b38595a05d5b928e42e841ed47",
    });
    expect(source.hosted["v0.6.2"]).toEqual({
      ref: "c71f820c93627816fbb892160e3b48323b81536a",
      sha256: "cfb3ee4a11d17ec817c55d73f3aaf8cbb97c40822555a75c2576b9bac3274f87",
    });
    expect(source.hosted["v0.7.0"]).toEqual({
      ref: "76f6ae83356ab726a816e9ee07992dd54b9a30ec",
      sha256: "cfb3ee4a11d17ec817c55d73f3aaf8cbb97c40822555a75c2576b9bac3274f87",
    });
    expect(source.hosted["v0.9.0"]).toEqual({
      ref: "889acba094fa1b4a7c43b7ad0bcb9fab8e642c04",
      sha256: "cfb3ee4a11d17ec817c55d73f3aaf8cbb97c40822555a75c2576b9bac3274f87",
    });
    expect(source.hosted["v0.10.0"]).toEqual({
      ref: "f2b6a688c1b07e46ac92a9ddab2595d48262c3df",
      sha256: "2fb956cbc06698ea8672db813a9961a91e19a11fd232a08d61b2a8e3bfb1cf91",
    });
    expect(source.hosted["v0.11.0"]).toEqual({
      ref: "2e678a203c607779a50271fabcb9534ddc94ce06",
      sha256: "ff1bceb19e4a6688be7d8bd5c1e8a7cf1e2633fc35a3441b17938715d3aeb878",
    });
    expect(source.hosted["v0.12.0"]).toEqual({
      ref: "5a72442649f6958e91a50283a91e985f45e10a09",
      sha256: "ff1bceb19e4a6688be7d8bd5c1e8a7cf1e2633fc35a3441b17938715d3aeb878",
    });
    // hull v0.12.1 hosts the same bytes as v0.12.0 (tag v0.12.1 -> f59ed2b).
    expect(source.hosted["v0.12.1"]).toEqual({
      ref: "f59ed2bb7c30acdffc8fb14ba39273578bdb915a",
      sha256: "ff1bceb19e4a6688be7d8bd5c1e8a7cf1e2633fc35a3441b17938715d3aeb878",
    });
    // hull v0.13.0 hosts a new ready.mjs (tag v0.13.0 -> 3f4a65f).
    expect(source.hosted["v0.13.0"]).toEqual({
      ref: "3f4a65fda568cdd20c76168ed156d25a28ba5630",
      sha256: "189a681d40ad3cdd243e4183bda9d9774ebada31a2563f0968211827d9acb16e",
    });
    for (const [tag, { sha256 }] of Object.entries(source.hosted)) {
      expect(sha(hosted(tag))).toBe(sha256);
    }
  });

  it("is the copy without its H1, after a Use Claude prompt that pins the sha256", () => {
    // Verbatim except the hosted assess URL, its sha256 and hull tag, which render from versions.hull.
    const shown = source.hosted[versions.hull]!.sha256;
    const guide = copy
      .replace(/^# .*\n\n/, "")
      .replace(
        /https:\/\/keelstack\.dev\/assess\/v\d+\.\d+\.\d+\.mjs/g,
        `https://keelstack.dev/assess/${versions.hull}.mjs`,
      )
      .replace(/[0-9a-f]{64}(  \/tmp\/assess\.mjs)/g, `${shown}$1`)
      .replace(
        /(hull )v\d+\.\d+\.\d+('s `template\/pieces\/base\/files\/scripts\/ready\.mjs`)/g,
        `$1${versions.hull}$2`,
      );
    expect(page.endsWith(guide)).toBe(true);
    const prompt = page.split("```text\n")[1]?.split("```")[0] ?? "";
    expect(prompt).toContain(`https://keelstack.dev/assess/${versions.hull}.mjs`);
    expect(prompt).toContain("hull.assess/1");
    expect(prompt).toContain(
      `${source.hosted[versions.hull]!.sha256}  /tmp/assess.mjs" | sha256sum -c`,
    );
    expect(prompt).toContain("node /tmp/assess.mjs --assess --json");
    expect(prompt).toContain(`@keelstackhq/splice@${versions.splice} adopt`);
    expect(prompt).toContain("--dry-run --json");
    expect(prompt).toContain("--force");
    expect(prompt).not.toMatch(/latest/);
    expect(prompt).toContain("keeping an owned base file (for example ci.yml) blocks adopt");
  });

  it("names no assess tag or sha256 other than the shown hull's, in any built docs page", () => {
    const dirs = [
      new URL("../pages/docs/", import.meta.url),
      new URL("../../../../agents/", import.meta.url),
    ];
    const files = dirs.flatMap((d) =>
      readdirSync(d)
        .filter((f) => f.endsWith(".md"))
        .map((f) => new URL(f, d)),
    );
    for (const f of files) {
      const text = readFileSync(f, "utf8");
      for (const m of text.matchAll(/keelstack\.dev\/assess\/(v[\d.]+)\.mjs/g)) {
        expect(m[1], f.pathname).toBe(versions.hull);
      }
      for (const m of text.matchAll(/([0-9a-f]{64})  \/tmp\/assess\.mjs/g)) {
        expect(m[1], f.pathname).toBe(source.hosted[versions.hull]!.sha256);
      }
    }
  });

  it("says TODO(owner) is hull's marker, not a step", () => {
    expect(page).toContain("`TODO(owner)` in the copy is hull's own marker");
  });

  it("is a ready docs page after Greenfield repo with bosun", () => {
    const slugs = docs.map((d) => d.slug);
    expect(slugs.indexOf("adopt")).toBe(slugs.indexOf("greenfield-bosun") + 1);
    expect(docs.find((d) => d.slug === "adopt")?.sources).toContain("hull/docs/adopt.md");
  });
});
