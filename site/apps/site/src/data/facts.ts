// Every fact the landing page shows, read from the monorepo sources at build time (D-024).
// Nothing here is typed in by hand: the workspace version, the tool list, each tool's one-line
// description, its subcommands, npm package, hull's pieces and the stack versions all come from
// files in this repository. A change to any of them changes the page on the next build, and the
// deploy-site workflow rebuilds on every change to those folders. `facts.test.ts` fails when the
// rendered page shows a value that is not in this object.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export interface Subcommand {
  name: string;
  doc: string;
}

export interface ToolFact {
  name: string;
  /** One line: the crate's `description` (hull: the first sentences of its README). */
  description: string;
  /** Subcommands from the `Command` enum of the crate (empty for hull). */
  subcommands: Subcommand[];
  /** Name of the npm launcher package, when the folder has one. */
  npm?: string;
  /** Cargo package name (`cargo install <crate>`). */
  crate?: string;
  /** `https://github.com/<org>/<repo>/tree/main/<folder>` */
  url: string;
}

export interface PieceFact {
  /** `template` or `template-rust` */
  template: string;
  id: string;
  description: string;
  /** Default instance name: the `name` var's default, else the last part of `target`, else the id. */
  instance: string;
  multiple: boolean;
}

export interface Facts {
  /** `[workspace.package] version` of the root Cargo.toml: one version for every tool. */
  version: string;
  tag: string;
  repoUrl: string;
  orgUrl: string;
  repoSlug: string;
  /** `gh:<org>/<repo>//hull/template` */
  hullSource: string;
  tools: ToolFact[];
  pieces: PieceFact[];
  stack: { node: string; pnpm: string; typescript: string };
  /** Piece counts of the default template. */
  templateFiles: Record<string, number>;
  hoardAction: string;
}

const here = dirname(fileURLToPath(import.meta.url));

/** The monorepo root: the nearest parent whose Cargo.toml has a `[workspace]` table. */
export function findRoot(from: string = here): string {
  let dir = from;
  for (;;) {
    const p = join(dir, "Cargo.toml");
    if (existsSync(p) && /^\[workspace\]/m.test(readFileSync(p, "utf8"))) return dir;
    const up = dirname(dir);
    if (up === dir) throw new Error(`facts: no Cargo workspace above ${from}`);
    dir = up;
  }
}

/** `key = "value"` inside one `[section]` (section `""` is the top of the file). */
export function tomlString(src: string, section: string, key: string): string | undefined {
  let cur = "";
  for (const line of src.split("\n")) {
    const h = /^\s*\[([^\]]+)\]\s*(#.*)?$/.exec(line);
    if (h) {
      cur = h[1]!.trim();
      continue;
    }
    if (cur !== section) continue;
    const m = new RegExp(`^\\s*${key}\\s*=\\s*"((?:[^"\\\\]|\\\\.)*)"`).exec(line);
    if (m) return m[1]!.replace(/\\"/g, '"');
  }
  return undefined;
}

/** The `members = [...]` list of `[workspace]` (one line). */
export function workspaceMembers(src: string): string[] {
  const m = /^members\s*=\s*\[([^\]]*)\]/m.exec(src);
  if (!m) throw new Error("facts: [workspace] members not found");
  return [...m[1]!.matchAll(/"([^"]+)"/g)].map((x) => x[1]!);
}

const kebab = (s: string): string => s.replace(/([a-z0-9])([A-Z])/g, "$1-$2").toLowerCase();

/** Subcommands of the first `enum Command` in a clap source file, in source order. */
export function parseSubcommands(src: string): Subcommand[] {
  const start = /^(?:pub )?enum Command \{$/m.exec(src);
  if (!start) return [];
  const lines = src.slice(start.index + start[0].length).split("\n");
  const out: Subcommand[] = [];
  let doc = "";
  let attrs: string[] = [];
  for (const line of lines) {
    if (line === "}") break;
    const d = /^ {4}\/\/\/ ?(.*)$/.exec(line);
    if (d) {
      doc = doc ? `${doc} ${d[1]!}` : d[1]!;
      continue;
    }
    if (/^ {4}#\[/.test(line)) {
      attrs.push(line.trim());
      continue;
    }
    const v = /^ {4}([A-Z][A-Za-z0-9]*)\b/.exec(line);
    if (v) {
      const a = attrs.join(" ");
      if (!/hide\s*=\s*true/.test(a)) {
        const named = /name\s*=\s*"([^"]+)"/.exec(a);
        out.push({ name: named ? named[1]! : kebab(v[1]!), doc: doc.trim() });
      }
      doc = "";
      attrs = [];
    }
  }
  return out;
}

/** The first paragraph of a README after its `# title`, without links to the org and status lines. */
export function readmeLead(src: string): string {
  const paras = src.replace(/\r/g, "").split(/\n\s*\n/);
  const lead = paras.find((p) => !p.startsWith("#") && !p.startsWith(">") && p.trim() !== "");
  if (!lead) throw new Error("facts: README has no lead paragraph");
  const flat = lead.replace(/\s*\n\s*/g, " ").replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
  // Keep the sentences before "Part of ...".
  return flat.replace(/\s*Part of .*$/, "").trim();
}

function readPiece(template: string, dir: string): PieceFact {
  const src = readFileSync(join(dir, "piece.toml"), "utf8");
  const id = tomlString(src, "", "id");
  const description = tomlString(src, "", "description");
  if (!id || !description) throw new Error(`facts: ${dir}/piece.toml needs id and description`);
  const target = tomlString(src, "", "target");
  const nameVar = /^name\s*=\s*\{[^}]*default\s*=\s*"([^"]+)"/m.exec(src);
  return {
    template,
    id,
    description,
    instance: nameVar ? nameVar[1]! : target ? basename(target) : id,
    multiple: /^multiple\s*=\s*true/m.test(src),
  };
}

function countFiles(dir: string): number {
  if (!existsSync(dir)) return 0;
  let n = 0;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    n += e.isDirectory() ? countFiles(join(dir, e.name)) : 1;
  }
  return n;
}

export function loadFacts(root: string = findRoot()): Facts {
  const read = (...p: string[]): string => readFileSync(join(root, ...p), "utf8");
  const cargo = read("Cargo.toml");
  const version = tomlString(cargo, "workspace.package", "version");
  if (!version) throw new Error("facts: [workspace.package] version missing in Cargo.toml");
  const members = workspaceMembers(cargo);

  const first = members[0]!;
  const repoUrl = tomlString(read(first, "Cargo.toml"), "package", "repository");
  const m = /^(https:\/\/github\.com\/([^/]+))\/([^/]+?)(?:\.git)?\/?$/.exec(repoUrl ?? "");
  if (!m) throw new Error(`facts: ${first}/Cargo.toml needs a GitHub repository URL`);
  const [, orgUrl, org, repo] = m as unknown as [string, string, string, string];
  const base = `${orgUrl}/${repo}`;

  const tools: ToolFact[] = [];
  const hullReadme = readmeLead(read("hull", "README.md"));
  tools.push({
    name: "hull",
    description: hullReadme,
    subcommands: [],
    url: `${base}/tree/main/hull`,
  });
  for (const name of members) {
    const toml = read(name, "Cargo.toml");
    const description = tomlString(toml, "package", "description");
    if (!description) throw new Error(`facts: ${name}/Cargo.toml has no description`);
    const mainRs = ["src/cli/mod.rs", "src/cli.rs", "src/main.rs"]
      .map((p) => join(root, name, p))
      .find((p) => existsSync(p) && /enum Command \{/.test(readFileSync(p, "utf8")));
    const npmPkg = join(root, name, "npm", name, "package.json");
    tools.push({
      name,
      description,
      subcommands: mainRs ? parseSubcommands(readFileSync(mainRs, "utf8")) : [],
      crate: tomlString(toml, "package", "name"),
      npm: existsSync(npmPkg)
        ? (JSON.parse(readFileSync(npmPkg, "utf8")) as { name: string }).name
        : undefined,
      url: `${base}/tree/main/${name}`,
    });
  }

  const pieces: PieceFact[] = [];
  for (const template of ["template", "template-rust"]) {
    const dir = join(root, "hull", template, "pieces");
    if (!existsSync(dir)) continue;
    for (const id of readdirSync(dir).toSorted()) {
      if (existsSync(join(dir, id, "piece.toml"))) pieces.push(readPiece(template, join(dir, id)));
    }
  }

  const baseFiles = join(root, "hull", "template", "pieces", "base", "files");
  const node = readFileSync(join(baseFiles, ".node-version"), "utf8").trim();
  const pnpmM = /"packageManager":\s*"pnpm@([^"]+)"/.exec(
    readFileSync(join(baseFiles, "package.json.jinja"), "utf8"),
  );
  const tsM = /^\s*typescript:\s*([0-9.]+)/m.exec(
    readFileSync(join(baseFiles, "pnpm-workspace.yaml"), "utf8"),
  );
  if (!pnpmM || !tsM) throw new Error("facts: hull base has no pnpm or typescript pin");
  const templateFiles: Record<string, number> = {};
  for (const p of pieces.filter((x) => x.template === "template")) {
    templateFiles[p.id] = countFiles(join(root, "hull", "template", "pieces", p.id, "files"));
  }

  return {
    version,
    tag: `v${version}`,
    repoUrl: base,
    orgUrl,
    repoSlug: `${org}/${repo}`,
    hullSource: `gh:${org}/${repo}//hull/template`,
    tools,
    pieces,
    stack: { node, pnpm: pnpmM[1]!, typescript: tsM[1]! },
    templateFiles,
    hoardAction: `${org}/${repo}/hoard@v${version}`,
  };
}

export const facts: Facts = loadFacts();
