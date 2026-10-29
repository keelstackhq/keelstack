import { createHash } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { docHref, docs, readyDocs, sourceUrl } from "./docs";
import { tools } from "./tools";

const bosunVersion = tools.find((t) => t.name === "bosun")?.status.replace(/^v/, "");

const dir = new URL("../pages/docs/", import.meta.url);
const repos = ["hull", "splice", "bosun", "hoist", "hoard", "berth", "decisions", "rust-canary"];
const fileFor = (slug: string) => (slug === "" ? "index.md" : `${slug}.md`);
const files = readdirSync(dir).filter((f) => f.endsWith(".md"));

describe("docs data", () => {
  it("has the twelve pages in order with unique slugs", () => {
    expect(docs.map((d) => d.slug)).toEqual([
      "",
      "start-here",
      "start-project",
      "greenfield-bosun",
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
      "Start a project",
      "Greenfield repo with bosun",
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
  // bosun's docs/setup-script.sh, copied verbatim at a pinned bosun commit (data/bosun/).
  const copy = readFileSync(new URL("./bosun/setup-script.sh", import.meta.url), "utf8");
  const source = JSON.parse(
    readFileSync(new URL("./bosun/setup-script.json", import.meta.url), "utf8"),
  ) as { repo: string; ref: string; path: string; sha256: string };

  it("embeds bosun's setup script verbatim, from a pinned commit", () => {
    expect(source.repo).toBe("keelstackhq/bosun");
    expect(source.path).toBe("docs/setup-script.sh");
    expect(source.ref).toMatch(/^[0-9a-f]{40}$/);
    expect(createHash("sha256").update(copy).digest("hex")).toBe(source.sha256);
    expect(page.split("```bash\n")[1]?.split("```")[0]).toBe(copy);
    expect(page).toContain(
      `https://github.com/keelstackhq/bosun/blob/${source.ref}/docs/setup-script.sh`,
    );
  });

  it("pins the bosun release the site shows", () => {
    expect(copy).toContain(`BOSUN_VERSION=${bosunVersion}\n`);
  });
});

describe("docspack package", () => {
  const page = readFileSync(new URL("../pages/docs/getting-started.md", import.meta.url), "utf8");
  it("the Getting started page installs it from npm", () => {
    expect(page).toContain("pnpm add -D docspack@1.5.0 @keelstackhq/docspack\n");
    expect(page).not.toContain("keelstack.dev/docspack/");
  });
});

const src = (slug: string) => docs.find((d) => d.slug === slug)!.sources;

describe("sources after the 2026-10-28 refresh", () => {
  it("cites bosun's split spec, not the old single file", () => {
    for (const d of docs) expect(d.sources).not.toContain("bosun/docs/spec.md");
  });

  it("cites the CircleCI, actions and secrets sources", () => {
    expect(src("ci")).toEqual(
      expect.arrayContaining(["hull/docs/actions.md", "hoard/docs/circleci.md"]),
    );
    expect(src("releases")).toContain("hull/template/pieces/secrets/files/docs/agents/secrets.md");
  });
});

describe("Greenfield repo with bosun page", () => {
  const page = readFileSync(new URL("../pages/docs/greenfield-bosun.md", import.meta.url), "utf8");
  it("comes right after Start a project and is linked from it", () => {
    const slugs = docs.map((d) => d.slug);
    expect(slugs.indexOf("greenfield-bosun")).toBe(slugs.indexOf("start-project") + 1);
    const start = readFileSync(new URL("../pages/docs/start-project.md", import.meta.url), "utf8");
    expect(start).toContain("](/docs/greenfield-bosun/)");
  });
  it("shows a plan in bosun's format and the documented order", () => {
    for (const s of [
      "project: ",
      "### Tasks",
      "### Done when",
      "- UC-",
      "bosun plan check",
      "bosun plan import",
      "bosun wave plan",
      "bosun spawn",
    ])
      expect(page).toContain(s);
  });
});
