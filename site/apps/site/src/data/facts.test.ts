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
const ARCHIVED =
  /github\.com\/keelstackhq\/(hull|splice|hoard|bosun|hoist|berth|keel|decisions|site)\b|gh:keelstackhq\/(?!keelstack\/)[\w-]+\/\/|(?<![@\w])keelstackhq\/(splice|hoard|hoist|bosun)(\/[\w-]+)?@/;
// History lines that name an archived source on purpose (keep this list small).
const HISTORY = [
  // troubleshooting.md: a fix for repos rendered from the old hull repo's v0.1.x.
  "### `splice channel add gh:keelstackhq/hull//rigging` refuses in a repo from hull v0.1.x",
];
// History: verbatim copies, each checked against its sha256 (adopt, start-checklist, docs tests).
// A docs line that is a line of one of them may name what the copy names.
const COPIES = [
  "src/data/hull/adopt.md",
  "src/data/hull/start-checklist.md",
  "src/data/bosun/setup-script.sh",
]
  .map((p) => read(p))
  .concat(HISTORY)
  .join("\n");
const VERBATIM = new Set(COPIES.split("\n"));
/** Values `re` finds in the verbatim copies: allowed in the built docs (history). */
const inCopies = (re: RegExp): Set<string> =>
  new Set([...decode(COPIES).matchAll(re)].map((m) => m[1]!));

describe("no hand-typed fact in the docs sources", () => {
  // History lives in one file, src/data/legacy.ts (the archived repos' last releases); not scanned.
  const docsSrc = walk(join(appDir, "docs-src"))
    .filter((f) => f.endsWith(".md"))
    .map((f) => relative(appDir, f));
  const tsFiles = [
    "src/data/versions.ts",
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
/** `[file, first group]` for every match of `re` in the build. */
const all = (re: RegExp): [string, string][] =>
  builtFiles().flatMap(([f, src]) =>
    [...src.matchAll(re)].map((m) => [f, m[1]!] as [string, string]),
  );

describe("the built docs show only commands that resolve", () => {
  test("every npm pin is the pinned version", () => {
    const re = /@keelstackhq\/(?:splice|bosun|hoist|berth)@(<new>|[^\s`'"),\]<]+)/g;
    const pins = all(re);
    expect(pins.length).toBeGreaterThan(10);
    const ok = new Set([...Object.values(versions).map(bare), "<new>", ...inCopies(re)]);
    expect(pins.filter(([, v]) => !ok.has(v.replace(/[.:;]+$/, "")))).toEqual([]);
  });

  test("every --ref is the pinned hull ref or a placeholder", () => {
    const re = /--ref (v[0-9][^\s`'"),]*|vX\.Y\.Z)/g;
    const refs = all(re);
    expect(refs.length).toBeGreaterThan(3);
    const ok = new Set([versions.hull, "vX.Y.Z", ...inCopies(re)]);
    expect(refs.filter(([, r]) => !ok.has(r))).toEqual([]);
  });

  test("every gh: source is the pinned hull or decisions source", () => {
    const re = /(gh:[\w.-]+\/[\w.-]+\/\/(?:[\w./-]|<template>)*)/g;
    const srcs = all(re);
    expect(srcs.length).toBeGreaterThan(5);
    const copied = inCopies(re);
    const ok = (s: string): boolean =>
      s.startsWith(`${sources.hull}/`) ||
      s === sources.decisions ||
      s === sources.bosunChannel ||
      copied.has(s);
    expect(srcs.filter(([, s]) => !ok(s))).toEqual([]);
  });

  test("every keelstack action is the pinned one", () => {
    const uses = all(/uses: (keelstackhq\/[\w/-]+@[\w.]+)/g);
    expect(uses.length).toBeGreaterThan(0);
    const ok = new Set([sources.spliceActionUses, sources.hoardAction, sources.hoardMajor]);
    expect(uses.filter(([, u]) => !ok.has(u))).toEqual([]);
  });

  test("every GitHub link points at an existing monorepo path or the org", () => {
    const re = /(https:\/\/github\.com\/[^\s)"'`<>\]]+)/g;
    const copied = inCopies(re);
    const urls = all(re).filter(([, u]) => !copied.has(u));
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
});
