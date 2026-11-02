// "Adopt an existing repo" (/docs/adopt/) is hull's docs/adopt.md, copied verbatim at a pinned hull
// ref, and keelstack.dev hosts hull's ready.mjs (the assessment) at /assess/<tag>.mjs from the
// same ref. Re-copy both with apps/site/scripts/adopt.mjs.
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docs } from "./docs";

const read = (rel: string) => readFileSync(new URL(rel, import.meta.url), "utf8");
const sha = (s: string) => createHash("sha256").update(s).digest("hex");
const source = JSON.parse(read("./hull/adopt.json")) as {
  repo: string;
  ref: string;
  doc: { path: string; sha256: string };
  assess: { path: string; url: string; sha256: string };
};
const copy = read("./hull/adopt.md");
const assess = read("./hull/ready.mjs.txt");
const page = read("../pages/docs/adopt.md");

describe("Adopt an existing repo", () => {
  it("pins hull's adopt.md and ready.mjs to one commit", () => {
    expect(source.repo).toBe("keelstackhq/hull");
    // hull v0.6.0: refs/tags/v0.6.0^{}.
    expect(source.ref).toBe("9b4a0e44e10470d5aa80cc07052440cfa92a7496");
    expect(source.doc.path).toBe("docs/adopt.md");
    expect(sha(copy)).toBe(source.doc.sha256);
    expect(source.assess.path).toBe("template/pieces/base/files/scripts/ready.mjs");
  });

  it("hosts the assessment at a versioned URL with the expected sha256", () => {
    expect(source.assess.url).toBe("/assess/v0.6.0.mjs");
    expect(sha(assess)).toBe("c425300387529849b84c04bcb3fb6a38aba4de920c84fdf7fe792e7e2fb4691d");
    expect(source.assess.sha256).toBe(sha(assess));
  });

  it("is the copy without its H1, after a Use Claude prompt that checks the sha256", () => {
    expect(page.endsWith(copy.replace(/^# .*\n\n/, ""))).toBe(true);
    const prompt = page.split("```text\n")[1]?.split("```")[0] ?? "";
    expect(prompt).toContain("https://keelstack.dev/assess/v0.6.0.mjs");
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
