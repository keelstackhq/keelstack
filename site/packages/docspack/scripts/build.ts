// Packs the site's generated Markdown (apps/site/dist/docs/*.md, the same text as /llms-full.txt)
// as this docspack package: .llms/ and llms.txt here, named and versioned by package.json.
// turbo builds @keelstackhq/site first (turbo.json).
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { missingPages } from "../src/pages.ts";

const root = join(import.meta.dirname, "..");
const site = join(root, "../../apps/site/dist/docs");
const pagesDir = join(root, "dist/pages");

if (!existsSync(site))
  throw new Error(`docspack: ${site} is missing; build @keelstackhq/site first`);
const pages = readdirSync(site)
  .filter((f) => f.endsWith(".md"))
  .toSorted();
if (pages.length === 0) throw new Error(`docspack: no Markdown pages in ${site}`);

rmSync(pagesDir, { recursive: true, force: true });
mkdirSync(pagesDir, { recursive: true });
for (const page of pages) copyFileSync(join(site, page), join(pagesDir, page));

// Name, version and `from` come from package.json.
execFileSync("docspack", ["build", "-q"], { cwd: root, stdio: "inherit" });

const chunksDir = join(root, ".llms/chunks");
const chunks = readdirSync(chunksDir).map((f) => readFileSync(join(chunksDir, f), "utf8"));
const missing = missingPages(pages, chunks);
if (missing.length > 0)
  throw new Error(`docspack: pages missing from the pack: ${missing.join(", ")}`);
console.log(`docspack: ${pages.length} pages, ${chunks.length} chunks`);
