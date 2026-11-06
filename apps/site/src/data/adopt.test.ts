// "Adopt an existing repo" (/docs/adopt/) is hull's docs/adopt.md, copied verbatim at a pinned hull
// ref, and keelstack.dev hosts hull's ready.mjs (the assessment) at /assess/<tag>.mjs from the
// same ref. Older tags stay hosted, since old links pin them. Re-copy with apps/site/scripts/adopt.mjs.
import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docs } from "./docs";

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
    expect(source.repo).toBe("keelstackhq/hull");
    // hull v0.7.0 tag commit (refs/tags/v0.7.0^{}).
    expect(source.ref).toBe("76f6ae83356ab726a816e9ee07992dd54b9a30ec");
    expect(source.doc.path).toBe("docs/adopt.md");
    expect(sha(copy)).toBe(source.doc.sha256);
    expect(source.assess.path).toBe("template/pieces/base/files/scripts/ready.mjs");
  });

  it("hosts the assessment at a versioned URL with the expected sha256", () => {
    expect(source.assess.url).toBe("/assess/v0.7.0.mjs");
    expect(sha(hosted("v0.7.0"))).toBe(
      "cfb3ee4a11d17ec817c55d73f3aaf8cbb97c40822555a75c2576b9bac3274f87",
    );
    expect(source.assess.sha256).toBe(sha(hosted("v0.7.0")));
  });

  it("keeps every earlier assessment served byte for byte", () => {
    expect(Object.keys(source.hosted).toSorted()).toEqual(["v0.6.0", "v0.6.1", "v0.6.2", "v0.7.0"]);
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
    expect(source.hosted["v0.7.0"]).toEqual({ ref: source.ref, sha256: source.assess.sha256 });
    for (const [tag, { sha256 }] of Object.entries(source.hosted)) {
      expect(sha(hosted(tag))).toBe(sha256);
    }
  });

  it("is the copy without its H1, after a Use Claude prompt that checks the sha256", () => {
    expect(page.endsWith(copy.replace(/^# .*\n\n/, ""))).toBe(true);
    const prompt = page.split("```text\n")[1]?.split("```")[0] ?? "";
    expect(prompt).toContain("https://keelstack.dev/assess/v0.7.0.mjs");
    expect(prompt).toContain("hull.assess/1");
    expect(prompt).toContain(`${source.assess.sha256}  /tmp/assess.mjs" | sha256sum -c`);
    expect(prompt).toContain("node /tmp/assess.mjs --assess --json");
    expect(prompt).toContain("@keelstackhq/splice@0.4.0 adopt");
    expect(prompt).toContain("--dry-run --json");
    expect(prompt).toContain("--force");
    expect(prompt).not.toMatch(/latest/);
    expect(prompt).toContain("keeping an owned base file (for example ci.yml) blocks adopt");
  });

  it("is a ready docs page after Greenfield repo with bosun", () => {
    const slugs = docs.map((d) => d.slug);
    expect(slugs.indexOf("adopt")).toBe(slugs.indexOf("greenfield-bosun") + 1);
    expect(docs.find((d) => d.slug === "adopt")?.sources).toContain("hull/docs/adopt.md");
  });
});
