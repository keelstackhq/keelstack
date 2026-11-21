import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, test } from "vite-plus/test";
import {
  facts,
  findRoot,
  parseLegacyTags,
  parseSubcommands,
  readmeLead,
  semverGt,
  tomlString,
  workspaceMembers,
} from "./facts";
import { terminal } from "./content";
import { readyDocs } from "./docs";
import { tools } from "./tools";
import { bare, links, sources, versions } from "./versions";

const root = new URL("../../", import.meta.url);
const decode = (s: string): string =>
  s
    .replace(/&#(\d+);/g, (_, n: string) => String.fromCharCode(Number(n)))
    .replace(/&amp;/g, "&")
    .replace(/&quot;/g, '"');
const built = (p: string): boolean => {
  const rel = p.replace(/^\//, "");
  const candidates = [rel, `${rel}${rel === "" || rel.endsWith("/") ? "" : "/"}index.html`];
  return candidates.some((c) => {
    try {
      readFileSync(new URL(`dist/${c}`, root));
      return true;
    } catch {
      return false;
    }
  });
};
const read = (p: string): string => readFileSync(new URL(p, root), "utf8");

describe("parsers", () => {
  test("tomlString reads a key of one section only", () => {
    const src = '[package]\nname = "a"\ndescription = "x \\"y\\""\n[other]\nname = "b"\n';
    expect(tomlString(src, "package", "name")).toBe("a");
    expect(tomlString(src, "package", "description")).toBe('x "y"');
    expect(tomlString(src, "other", "name")).toBe("b");
    expect(tomlString(src, "none", "name")).toBeUndefined();
  });

  test("workspaceMembers", () => {
    expect(workspaceMembers('[workspace]\nmembers = ["a", "b"]\n')).toEqual(["a", "b"]);
  });

  test("parseSubcommands: kebab names, name override, hidden skipped, docs joined", () => {
    const src = [
      "pub enum Command {",
      "    /// Make it.",
      "    /// Twice.",
      "    RiskCheck(A),",
      '    #[command(name = "handoff-pr")]',
      "    HandoffPr(B),",
      "    #[command(hide = true)]",
      "    Secret,",
      "    #[command(subcommand)]",
      "    Wave(C),",
      "}",
      "enum Other {",
      "    Nope,",
      "}",
    ].join("\n");
    expect(parseSubcommands(src)).toEqual([
      { name: "risk-check", doc: "Make it. Twice." },
      { name: "handoff-pr", doc: "" },
      { name: "wave", doc: "" },
    ]);
  });

  test("readmeLead drops the title, links and the 'Part of' tail", () => {
    const src =
      "# t\n\nAn [x](http://a) thing.\nSecond line. Part of [k](http://k): more.\n\n> status\n";
    expect(readmeLead(src)).toBe("An x thing. Second line.");
  });
});

describe("release state", () => {
  test("semverGt compares numerically", () => {
    expect(semverGt("0.22.0", "0.21.0")).toBe(true);
    expect(semverGt("0.21.10", "0.21.9")).toBe(true);
    expect(semverGt("1.0.0", "0.99.99")).toBe(true);
    expect(semverGt("0.21.0", "0.21.0")).toBe(false);
    expect(semverGt("0.20.9", "0.21.0")).toBe(false);
  });

  test("parseLegacyTags reads the floor and the tag -> commit lines", () => {
    const a = "a".repeat(40);
    const b = "b".repeat(40);
    const src = `# a\n# Releases (vX.Y.Z tags, > 1.2.3) come after.\nv0.1.0 ${a}\nv0.2.0 ${b}\n`;
    expect(parseLegacyTags(src)).toEqual({
      moveVersion: "1.2.3",
      tags: { "v0.1.0": a, "v0.2.0": b },
    });
    expect(() => parseLegacyTags("v0.1.0 abc\n")).toThrow(/release floor/);
  });

  test("released: the workspace version is above the move version (hull/legacy-tags.txt)", () => {
    expect(facts.moveVersion).toMatch(/^\d+\.\d+\.\d+$/);
    expect(facts.released).toBe(semverGt(facts.version, facts.moveVersion));
    expect(Object.keys(facts.hullLegacyTags).length).toBeGreaterThan(5);
  });
});

describe("facts come from the monorepo", () => {
  const rootDir = findRoot();
  const file = (p: string): string => readFileSync(`${rootDir}/${p}`, "utf8");

  test("version is the workspace version", () => {
    expect(facts.version).toBe(tomlString(file("Cargo.toml"), "workspace.package", "version"));
    expect(facts.version).toMatch(/^\d+\.\d+\.\d+$/);
  });

  test("every tool carries a crate description, subcommands and a folder link", () => {
    for (const t of facts.tools) {
      expect(t.description.length, t.name).toBeGreaterThan(10);
      expect(t.url).toBe(`${facts.repoUrl}/tree/main/${t.name}`);
      if (t.name !== "hull") expect(t.subcommands.length, t.name).toBeGreaterThan(2);
    }
    expect(facts.tools.find((t) => t.name === "splice")!.subcommands.map((s) => s.name)).toContain(
      "update",
    );
  });

  test("every npm launcher package mirrors the workspace version (lockstep)", () => {
    for (const t of facts.tools.filter((x) => x.npm)) {
      const pkg = JSON.parse(file(`${t.name}/npm/${t.name}/package.json`)) as { version: string };
      expect(pkg.version, t.name).toBe(facts.version);
    }
  });

  test("source URLs name the monorepo, never an archived repo", () => {
    expect(facts.hullSource).toBe("gh:keelstackhq/keelstack//hull/template");
    expect(facts.hoardAction).toBe(`keelstackhq/keelstack/hoard@v${facts.version}`);
    expect(facts.decisionsChannel).toBe("gh:keelstackhq/keelstack//decisions/channel");
    expect(facts.spliceAction).toBe("keelstackhq/keelstack/splice/action");
  });
});

describe("the rendered home page shows only what facts say", () => {
  let html = "";
  beforeAll(() => {
    // dist is rebuilt once per test run by test/build-once.ts (a stale dist must not make this pass).
    html = readFileSync(new URL("dist/index.html", root), "utf8");
  });

  test("every tool: name, version, description, detail and link are on the page", () => {
    const page = decode(html);
    for (const t of tools) {
      expect(page, t.name).toContain(t.pitch);
      expect(page, t.name).toContain(t.detail);
      expect(html, t.name).toContain(`href="${t.repo}"`);
    }
    expect(page).toContain(facts.tag);
  });

  test("the hero command and rows are the generated ones", () => {
    const page = decode(html);
    for (const line of terminal.command) expect(page).toContain(line);
    expect(page).toContain(terminal.summary);
  });

  test("every semver on the page is the workspace version", () => {
    const text = decode(html.replace(/<(script|style)[\s\S]*?<\/\1>/g, ""));
    const found = new Set([...text.matchAll(/\bv?(\d+\.\d+\.\d+)\b/g)].map((m) => m[1]));
    // The stack pins (pnpm) come from hull's template and are checked below. Commands pin
    // versions.ts (the workspace version once released, else the last releases that resolve).
    found.delete(facts.version);
    for (const v of Object.values(versions)) found.delete(bare(v));
    found.delete(facts.stack.pnpm);
    expect([...found]).toEqual([]);
  });

  test("every GitHub link points at the monorepo or the org", () => {
    const hrefs = [...html.matchAll(/href="(https:\/\/github\.com\/[^"]*)"/g)].map((m) => m[1]!);
    expect(hrefs.length).toBeGreaterThan(5);
    for (const h of hrefs) {
      expect(
        h === facts.orgUrl || h === facts.repoUrl || h.startsWith(`${facts.repoUrl}/`),
        h,
      ).toBe(true);
    }
  });

  test("internal links resolve to a built page or an anchor on this page", () => {
    const hrefs = [...html.matchAll(/href="(\/[^"#]*)(#[^"]*)?"/g)].map((m) => m[1]!);
    for (const h of new Set(hrefs)) expect(built(h), h).toBe(true);
    for (const a of html.matchAll(/href="#([^"]+)"/g)) {
      expect(html, a[1]).toContain(`id="${a[1]}"`);
    }
  });
});

describe("no hand-typed fact in the home page sources", () => {
  // The home page is built from these. Versions, repo URLs and stack pins must come from facts.ts.
  const dirs = ["src/components", "src/layouts"];
  const files = [
    ...dirs.flatMap((d) =>
      readdirSync(new URL(`${d}/`, root))
        .filter((f) => /\.(astro|tsx)$/.test(f) && !f.includes(".test."))
        .map((f) => `${d}/${f}`),
    ),
    "src/data/content.ts",
    "src/data/tools.ts",
    "src/lib/command.ts",
    "src/lib/picker-data.ts",
    "src/pages/index.astro",
  ];

  test.each(files)("%s has no semver literal", (f) => {
    const src = read(f).replace(/^\s*(\/\/|\*|\/\*).*$/gm, "");
    // Allowed: version floors of the docs layer in content.ts (update notes), Node majors are generated.
    const allowed =
      f === "src/data/content.ts" ? [/splice 0\.3\.0 or later/, /bosun 0\.11\.0/] : [];
    let text = src;
    for (const a of allowed) text = text.replace(a, "");
    expect(text.match(/\bv?\d+\.\d+\.\d+\b/g) ?? []).toEqual([]);
  });

  test.each(files)("%s names no archived tool repo", (f) => {
    const src = read(f);
    for (const line of src.split("\n")) {
      expect(line).not.toMatch(
        /github\.com\/keelstackhq\/(hull|splice|hoard|bosun|hoist|berth|keel)\b/,
      );
      expect(line).not.toMatch(/gh:keelstackhq\/(hull|decisions)\b/);
    }
  });
});

// --- /docs, /llms.txt, /agents.md and the scenario pages (D-024) ---

const appDir = fileURLToPath(root);
const walk = (d: string): string[] =>
  readdirSync(d, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? walk(join(d, e.name)) : [join(d, e.name)],
  );
// The archived repos (D-023). Only legacy.ts may name them; the docs get them through tokens.
const OLD = "(?:bosun|splice|hull|hoist|hoard|berth|keel|decisions|site)";
const ARCHIVED = new RegExp(
  [
    `github\\.com\\/keelstackhq\\/${OLD}\\b`,
    `raw\\.githubusercontent\\.com\\/keelstackhq\\/${OLD}\\b`,
    `gh:keelstackhq\\/(?!keelstack\\/)[\\w-]+\\/`,
    // Action refs and repo slugs, with or without `@<ref>` (not npm's `@keelstackhq/<tool>`).
    `(?<![@\\w./-])keelstackhq\\/${OLD}\\b`,
  ].join("|"),
);
// History lines that name an archived source on purpose (keep this list small), by output page.
const HISTORY: Record<string, string> = {
  // troubleshooting.md: a fix for repos rendered from the old hull repo's v0.1.x.
  troubleshooting:
    "### `splice channel add gh:keelstackhq/hull//rigging` refuses in a repo from hull v0.1.x",
};
// History: verbatim copies, each checked against its sha256 (adopt, start-checklist, docs tests),
// by the docs page that embeds them. A line of one of them may name what the copy names, and the
// built page (and its section of llms-full.txt) may show the copy's own pins and links.
const COPY_PAGES: Record<string, string> = {
  adopt: "src/data/hull/adopt.md",
  "start-project": "src/data/hull/start-checklist.md",
  cloud: "src/data/bosun/setup-script.sh",
};
const COPY_TEXT: Record<string, string> = Object.fromEntries(
  [...new Set([...Object.keys(COPY_PAGES), ...Object.keys(HISTORY)])].map((slug) => [
    slug,
    [COPY_PAGES[slug] ? read(COPY_PAGES[slug]) : "", HISTORY[slug] ?? ""].join("\n"),
  ]),
);
const VERBATIM = new Set(Object.values(COPY_TEXT).flatMap((t) => t.split("\n")));
/** Values `re` finds in the verbatim copies of page `slug`: allowed on that page only (history). */
const inCopies = (slug: string | undefined, re: RegExp): Set<string> =>
  new Set(
    slug && COPY_TEXT[slug] ? [...decode(COPY_TEXT[slug]).matchAll(re)].map((m) => m[1]!) : [],
  );

// docs-src lines that state a version as history in a form the prose scan does not recognise.
const HISTORY_SEMVER: string[] = [
  "0.2.0 both CIs ran everything", // ci.md: "(In" ends the line before.
  "records `0.0.0+g<sha>`", // getting-started.md: splice's placeholder version.
  "as a placeholder `0.0.0`", // releases.md: the npm placeholder.
  "(for example `cli-v1.2.0`)", // releases.md: an example tag.
  "pnpm add -D docspack@1.5.0", // getting-started.md: the third-party docspack CLI (docs.test.ts).
  "`2026.10.27` on npm is an older build", // getting-started.md: an old docspack build.
  'version (such as "bosun 0.4.0")', // start-project.md: names the copy's own example.
  "bosun 0.2.3's channel still", // getting-started.md: what the old channel did.
];

describe("no hand-typed fact in the docs sources", () => {
  // History lives in one file, src/data/legacy.ts (the archived repos' last releases); not scanned.
  const docsSrc = walk(join(appDir, "docs-src"))
    .filter((f) => f.endsWith(".md"))
    .map((f) => relative(appDir, f));
  const tsFiles = [
    "src/data/versions.ts",
    "src/data/derive.ts",
    "src/data/scenarios.ts",
    "src/data/docs.ts",
    "src/data/migrate.ts",
    "src/lib/llms.ts",
    "src/lib/render.ts",
    "src/lib/docs-md.ts",
    "src/lib/generate.ts",
  ];
  const handPages = ["src/pages/docs/playbook.md", "src/pages/docs/troubleshooting.md"];

  test.each([...tsFiles, ...docsSrc, ...handPages])("%s names no archived repo", (f) => {
    const hits = read(f)
      .split("\n")
      .filter((l) => ARCHIVED.test(l) && !VERBATIM.has(l));
    expect(hits).toEqual([]);
  });

  test.each(tsFiles)("%s has no semver literal", (f) => {
    // Allowed: version floors (history), never a pin.
    const allowed: Record<string, RegExp[]> = {
      "src/data/versions.ts": [/MIN_BOSUN_MIGRATE = "0\.15\.0"/],
      "src/data/scenarios.ts": [/\(0\.2\.1 or later\)/],
    };
    let text = read(f).replace(/^\s*(\/\/|\*|\/\*).*$/gm, "");
    for (const a of allowed[f] ?? []) text = text.replace(a, "");
    expect(text.match(/\bv?\d+\.\d+\.\d+\b/g) ?? []).toEqual([]);
  });

  test.each(docsSrc)("%s writes every token whole: {{ks.NAME}}", (f) => {
    // A formatter must not split a token (`{ { ks.x } }` renders as literal text).
    const hits = read(f)
      .split("\n")
      .filter((l) => /\{\s+\{|\}\s+\}|(?<![\w.-])ks\./.test(l.replace(/\{\{ks\.[\w.]+\}\}/g, "")));
    expect(hits).toEqual([]);
  });

  test.each(docsSrc)("%s states a version in prose only as history", (f) => {
    // A semver in docs-src prose is a token, a verbatim copy's line, or history: a version floor
    // or the release that changed something ("since bosun 0.10.0", "0.2.1 or later", "before
    // 0.2.2", "in 0.2.0"). What a page was checked against is a token (`checked.bosun`).
    const SEMVER = /\bv?\d+\.\d+\.\d+\b/g;
    const before =
      /(?:\b(?:since|before|after|until|from|in|to|older than|needs|added in)\s+|>=\s*|\(\s*)(?:(?:bosun|hull|splice|hoist|hoard|berth|npm)\s+)?$/i;
    // "<tool> X.Y.Z adds/ships/seeds ...": the release that introduced a feature.
    const after =
      /^(?:'s)?\s*(?:or later|and later|to v?\d|\(|\)|opt-in\b|(?:adds|ships|seeds|also|tries|ran|run|can run|and \w+ v?\d+\.\d+\.\d+ (?:can )?run)\b)/i;
    const hits: string[] = [];
    read(f)
      .split("\n")
      .forEach((l, i) => {
        if (VERBATIM.has(l)) return;
        for (const m of l.matchAll(SEMVER)) {
          const pre = l.slice(0, m.index);
          const post = l.slice(m.index + m[0].length);
          if (before.test(pre) || after.test(post) || HISTORY_SEMVER.some((h) => l.includes(h)))
            continue;
          hits.push(
            `${f}:${i + 1}: ${m[0]} in "${l.slice(Math.max(0, m.index - 40), m.index + 20)}"`,
          );
        }
      });
    expect(hits).toEqual([]);
  });

  test.each(docsSrc)("%s types no command pin", (f) => {
    const pin = /@keelstackhq\/[a-z-]+@\d|--ref v\d|\/assess\/v\d|@[0-9a-f]{40}\b/;
    expect(
      read(f)
        .split("\n")
        .filter((l) => pin.test(l) && !VERBATIM.has(l)),
    ).toEqual([]);
  });
});

/** Every built page and text file (not the hosted assess scripts or assets), decoded. */
const builtFiles = (): (readonly [string, string])[] =>
  walk(join(appDir, "dist"))
    .filter((f) => /\.(md|txt|html)$/.test(f) && !/[/\\](assess|_astro)[/\\]/.test(f))
    .map(
      (f) =>
        [
          relative(appDir, f),
          decode(readFileSync(f, "utf8")).replace(/&lt;/g, "<").replace(/&gt;/g, ">"),
        ] as const,
    );
/** `dist/docs/<slug>.md` and `dist/docs/<slug>/index.html` -> `<slug>`. */
const slugOf = (rel: string): string | undefined =>
  /^dist[/\\]docs[/\\](.+?)(?:[/\\]index\.html|\.md)$/.exec(rel)?.[1]?.replace(/\\/g, "/");
type Unit = readonly [file: string, slug: string | undefined, text: string];
/** The build in units of one docs page each: llms-full.txt is split into its pages' sections. */
const builtUnits = (): Unit[] =>
  builtFiles().flatMap(([f, src]): Unit[] => {
    if (!f.endsWith("llms-full.txt")) return [[f, slugOf(f), src]];
    const starts: [string, number][] = [];
    let pos = 0;
    for (const d of readyDocs()) {
      const i = src.indexOf(`\n# ${d.title}\n`, pos);
      if (i < 0) throw new Error(`llms-full.txt has no section "${d.title}"`);
      starts.push([d.slug, i]);
      pos = i + 1;
    }
    return [
      [f, undefined, src.slice(0, starts[0]![1])],
      ...starts.map(([s, i], k): Unit => [
        `${f}#${s}`,
        s,
        src.slice(i, starts[k + 1]?.[1] ?? src.length),
      ]),
    ];
  });
/** `[file, first group, page]` for every match of `re` in the build. */
const all = (re: RegExp): [string, string, string | undefined][] =>
  builtUnits().flatMap(([f, slug, src]) =>
    [...src.matchAll(re)].map((m) => [f, m[1]!, slug] as [string, string, string | undefined]),
  );

/** A pin without its sentence's trailing punctuation. */
const v0 = (v: string): string => v.replace(/[.:;]+$/, "");
/** The words of a text, as the archived-repo scan cuts them. */
const words = (text: string): Set<string> =>
  new Set(text.split(/[\s"'`<>()[\]|]+/).map((w) => w.replace(/[.,;:]+$/, "")));

describe("the built docs show only commands that resolve", () => {
  test("every npm pin is the pinned version", () => {
    const re = /@keelstackhq\/(?:splice|bosun|hoist|berth)@(<new>|[^\s`'"),\]<]+)/g;
    const pins = all(re);
    expect(pins.length).toBeGreaterThan(10);
    const ok = new Set([...Object.values(versions).map(bare), "<new>"]);
    expect(pins.filter(([, v, s]) => !ok.has(v0(v)) && !inCopies(s, re).has(v0(v)))).toEqual([]);
  });

  test("every --ref is the pinned hull ref or a placeholder", () => {
    const re = /--ref (v[0-9][^\s`'"),]*|vX\.Y\.Z)/g;
    const refs = all(re);
    expect(refs.length).toBeGreaterThan(3);
    const ok = new Set([versions.hull, "vX.Y.Z"]);
    expect(refs.filter(([, r, s]) => !ok.has(r) && !inCopies(s, re).has(r))).toEqual([]);
  });

  test("every gh: source is the pinned hull or decisions source", () => {
    const re = /(gh:[\w.-]+\/[\w.-]+\/\/(?:[\w./-]|<template>)*)/g;
    const srcs = all(re);
    expect(srcs.length).toBeGreaterThan(5);
    const ok = (s: string, page: string | undefined): boolean =>
      s.startsWith(`${sources.hull}/`) ||
      s === sources.decisions ||
      s === sources.bosunChannel ||
      inCopies(page, re).has(s);
    expect(srcs.filter(([, s, page]) => !ok(s, page))).toEqual([]);
  });

  test("every keelstack action is the pinned one", () => {
    const uses = all(/uses: (keelstackhq\/[\w/-]+@[\w.]+)/g);
    expect(uses.length).toBeGreaterThan(0);
    const ok = new Set([sources.spliceActionUses, sources.hoardAction, sources.hoardMajor]);
    expect(uses.filter(([, u]) => !ok.has(u))).toEqual([]);
  });

  test("every GitHub link points at an existing monorepo path or the org", () => {
    const re = /(https:\/\/github\.com\/[^\s)"'`<>\]]+)/g;
    const urls = all(re).filter(([, u, page]) => !inCopies(page, re).has(u));
    expect(urls.length).toBeGreaterThan(20);
    const rootDir = findRoot();
    const refs = new Set(["main", facts.tag, links.hullRef, links.scriptRef]);
    const bad = new Set<string>();
    for (const [f, u] of urls) {
      const url = u.replace(/[.,;:]+$/, "").replace(/#.*$/, "");
      if (url === facts.orgUrl || url === facts.repoUrl) continue;
      if (url.startsWith("https://github.com/apps/")) continue; // GitHub App install pages
      if (url.startsWith(`${facts.orgUrl}/rust-canary`)) continue; // not part of the monorepo
      const m = /^(.+?)\/(blob|tree)\/([^/]+)\/(.+)$/.exec(url);
      const ok =
        m !== null &&
        m[1] === facts.repoUrl &&
        refs.has(m[3]!) &&
        // An old pinned commit (hull's release, bosun's script copy) is checked by its own test.
        (!(m[3] === "main" || m[3] === facts.tag) || existsSync(join(rootDir, m[4]!)));
      if (!ok) bad.add(`${f}: ${u}`);
    }
    expect([...bad]).toEqual([]);
  });

  test("no build file names an archived repo, outside the verbatim copies' own pages", () => {
    // What legacy.ts holds reaches the build through tokens (versions.ts); anything else that
    // names an old repo must be a word of the copy embedded in that page (history).
    const word = /[^\s"'`<>()[\]|]*/y;
    const prefixes = [
      `${sources.hull}/`,
      sources.decisions,
      sources.bosunChannel,
      sources.spliceActionUses,
      sources.hoardAction,
      sources.hoardMajor,
      sources.hoistSecrets,
    ];
    const bad: string[] = [];
    for (const [f, slug, text] of builtUnits()) {
      const copy = slug && COPY_TEXT[slug] ? words(decode(COPY_TEXT[slug])) : new Set<string>();
      for (const m of text.matchAll(new RegExp(ARCHIVED.source, "g"))) {
        // The whole word around the match.
        let s = m.index;
        while (s > 0 && !/[\s"'`<>()[\]|]/.test(text[s - 1]!)) s--;
        word.lastIndex = s;
        const w = (word.exec(text)?.[0] ?? m[0]).replace(/[.,;:]+$/, "");
        const ok = w === sources.hullRepo || prefixes.some((p) => w.startsWith(p)) || copy.has(w);
        if (!ok) bad.push(`${f}: ${w}`);
      }
    }
    expect([...new Set(bad)]).toEqual([]);
  });

  test("no build file keeps an unrendered token (ks.NAME, {{ or a split { {)", () => {
    // Inline CSS and JS of the HTML pages are not docs text (their braces are code).
    const bad = builtUnits().flatMap(([f, , text]) =>
      [
        ...text
          .replace(/<(script|style)\b[\s\S]*?<\/\1>/g, "")
          .matchAll(/.{0,30}(?:\{\s*\{|\}\s*\}|(?<![\w.-])ks\.[A-Za-z]).{0,30}/g),
      ].map((m) => `${f}: ${m[0]}`),
    );
    expect(bad).toEqual([]);
  });
});
