#!/usr/bin/env node
// Build the @keelstackhq/docspack documentation package from the Markdown pages that `astro build`
// wrote to dist/docs/*.md (the same files as /llms-full.txt), check that every page is in it, and
// put its npm tarball at dist/docspack/<name>-<version>.tgz for https://keelstack.dev/docspack/.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;
const { name, version, sources } = JSON.parse(readFileSync(join(root, "docspack.json"), "utf8"));
const docs = join(root, "dist/docs");
const pages = readdirSync(docs).filter((f) => f.endsWith(".md"));
if (pages.length === 0) throw new Error("docspack: no dist/docs/*.md; run astro build first");

// A consumer's lockfile pins the tarball's integrity, so changed docs need a new version (and so a
// new file name); the same version must always pack the same Markdown.
const hash = createHash("sha256");
for (const f of pages.toSorted()) hash.update(`${f}\n`).update(readFileSync(join(docs, f)));
const digest = hash.digest("hex");
if (digest !== sources) {
  throw new Error(
    `docspack: the docs changed since ${name}@${version}. Bump "version" in apps/site/docspack.json ` +
      `(and the tarball URL in getting-started.md) and set "sources" to "${digest}".`,
  );
}

const work = mkdtempSync(join(tmpdir(), "keelstack-docspack-"));
try {
  const src = join(work, "src");
  const out = join(work, "out");
  mkdirSync(src);
  for (const f of pages) copyFileSync(join(docs, f), join(src, f));
  execFileSync(
    "docspack",
    ["build", "--from", src, "--name", name, "--pkg-version", version, "--out", out, "-q"],
    { stdio: "inherit" },
  );

  // Every page must be the source of at least one chunk.
  const chunks = readdirSync(join(out, ".llms/chunks")).map((f) =>
    readFileSync(join(out, ".llms/chunks", f), "utf8"),
  );
  const missing = pages.filter((p) => !chunks.some((c) => c.includes(`docspack: from ${p} -->`)));
  if (missing.length > 0)
    throw new Error(`docspack: pages missing from the pack: ${missing.join(", ")}`);

  const pkgPath = join(out, "package.json");
  const pkg = JSON.parse(readFileSync(pkgPath, "utf8"));
  pkg.description =
    "The keelstack docs (https://keelstack.dev/docs/) as a docspack documentation package.";
  pkg.homepage = "https://keelstack.dev/docs/";
  // npm checks provenance from trusted publishing against this URL.
  pkg.repository = { type: "git", url: "git+https://github.com/keelstackhq/keelstack.git" };
  writeFileSync(pkgPath, `${JSON.stringify(pkg, null, 2)}\n`);

  const dest = join(root, "dist/docspack");
  mkdirSync(dest, { recursive: true });
  execFileSync("npm", ["pack", "--silent", "--pack-destination", dest], {
    cwd: out,
    stdio: "inherit",
  });
  console.log(`docspack: ${name}@${version}, ${pages.length} pages, ${chunks.length} chunks`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
