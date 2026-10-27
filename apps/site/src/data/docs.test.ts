import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { SPLICE_VERSION } from "../lib/command";
import { docHref, docs, readyDocs, sourceUrl } from "./docs";
import { tools } from "./tools";

const bosunVersion = tools.find((t) => t.name === "bosun")?.status.replace(/^v/, "");

const dir = new URL("../pages/docs/", import.meta.url);
const repos = ["hull", "splice", "bosun", "hoist", "hoard", "berth", "decisions", "rust-canary"];
const fileFor = (slug: string) => (slug === "" ? "index.md" : `${slug}.md`);
const files = readdirSync(dir).filter((f) => f.endsWith(".md"));

describe("docs data", () => {
  it("has the ten pages in order with unique slugs", () => {
    expect(docs.map((d) => d.slug)).toEqual([
      "",
      "start-here",
      "getting-started",
      "cloud",
      "tasks",
      "ci",
      "risk",
      "releases",
      "playbook",
      "troubleshooting",
    ]);
    expect(docs.map((d) => d.title)).toEqual([
      "Overview",
      "Start here",
      "Getting started",
      "Cloud environment",
      "Tasks and sessions",
      "CI and gates",
      "Risk and review",
      "Releases and updates",
      "Playbook",
      "Troubleshooting",
    ]);
    expect(new Set(docs.map((d) => d.slug)).size).toBe(docs.length);
  });

  it("ready entries and page files match", () => {
    for (const d of readyDocs()) expect(files).toContain(fileFor(d.slug));
    const ready = new Set(readyDocs().map((d) => fileFor(d.slug)));
    for (const f of files) expect(ready.has(f)).toBe(true);
  });

  it("ready entries have a summary and valid sources", () => {
    for (const d of readyDocs()) {
      expect(d.summary.trim()).not.toBe("");
      expect(d.sources.length).toBeGreaterThan(0);
      for (const s of d.sources) {
        const [repo, ...rest] = s.split("/");
        expect(repos).toContain(repo);
        expect(rest.join("/")).not.toBe("");
      }
    }
  });

  it("builds hrefs and source urls", () => {
    expect(docHref("")).toBe("/docs/");
    expect(docHref("ci")).toBe("/docs/ci/");
    expect(sourceUrl("bosun/docs/spec.md")).toBe(
      "https://github.com/keelstackhq/bosun/blob/main/docs/spec.md",
    );
  });

  it("page files link only to ready pages and use no old org names", () => {
    const hrefs = new Set(readyDocs().map((d) => docHref(d.slug)));
    for (const f of files) {
      const text = readFileSync(new URL(f, dir), "utf8");
      for (const m of text.matchAll(/(?:\]\(|href=")(\/docs\/[^)"\s]*)/g)) {
        expect(hrefs.has(m[1]!.split("#")[0]!)).toBe(true);
      }
      for (const old of ["@keelstack/", "github.com/keelstack/", "gh:keelstack/"]) {
        expect(text).not.toContain(old);
      }
    }
  });
});

describe("cloud environment page", () => {
  const page = readFileSync(new URL("../pages/docs/cloud.md", import.meta.url), "utf8");
  it("embeds a setup script pinned to the versions the site shows", () => {
    const script = page.split("```bash\n")[1]?.split("```")[0] ?? "";
    expect(script).toContain(`SPLICE_VERSION=${SPLICE_VERSION}\n`);
    expect(script).toContain(`BOSUN_VERSION=${bosunVersion}\n`);
    expect(script).toContain('npm install -g "@keelstackhq/splice@${SPLICE_VERSION}"');
  });
});

describe("docspack package", () => {
  const meta = JSON.parse(
    readFileSync(new URL("../../docspack.json", import.meta.url), "utf8"),
  ) as {
    name: string;
    version: string;
  };
  it("is named and versioned like a docspack vendor package", () => {
    expect(meta.name).toBe("@keelstackhq/docspack");
    expect(meta.version).toMatch(/^\d{4}\.\d{1,2}\.\d{1,2}$/);
  });
  it("the Getting started page installs the current tarball", () => {
    const page = readFileSync(new URL("../pages/docs/getting-started.md", import.meta.url), "utf8");
    expect(page).toContain(
      `https://keelstack.dev/docspack/keelstackhq-docspack-${meta.version}.tgz`,
    );
  });
});
