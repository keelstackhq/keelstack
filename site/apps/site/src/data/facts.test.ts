import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { beforeAll, describe, expect, test } from "vite-plus/test";
import {
  facts,
  findRoot,
  parseSubcommands,
  readmeLead,
  tomlString,
  workspaceMembers,
} from "./facts";
import { terminal } from "./content";
import { tools } from "./tools";

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
  });
});

describe("the rendered home page shows only what facts say", () => {
  let html = "";
  beforeAll(() => {
    // Always rebuild: a stale dist must not make this pass.
    execFileSync("pnpm", ["run", "build"], { cwd: fileURLToPath(root) });
    html = readFileSync(new URL("dist/index.html", root), "utf8");
  }, 180_000);

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
    // The stack pins (pnpm) come from hull's template and are checked below.
    found.delete(facts.version);
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
    const allowed = f === "src/data/content.ts" ? /hull\/blob|bosun\/blob/ : /^$/;
    for (const line of src.split("\n")) {
      if (allowed.test(line)) continue;
      expect(line).not.toMatch(
        /github\.com\/keelstackhq\/(hull|splice|hoard|bosun|hoist|berth|keel)\b/,
      );
      expect(line).not.toMatch(/gh:keelstackhq\/(hull|decisions)\b/);
    }
  });
});
