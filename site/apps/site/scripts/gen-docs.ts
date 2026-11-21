// Writes the generated docs pages from docs-src/ (`pnpm --filter @keelstackhq/site gen`).
// `--check` renders and formats into a temp dir and exits 1 when a committed copy differs.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  diffGenerated,
  driftMessage,
  existingDocs,
  generate,
  outPath,
} from "../src/lib/generate.ts";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** Renders, then formats with `vp fmt` (tables re-align when a token changes width). */
function generateFormatted(): Map<string, string> {
  const map = generate();
  const tmp = mkdtempSync(join(tmpdir(), "gen-docs-"));
  try {
    for (const [path, content] of map) {
      mkdirSync(dirname(join(tmp, path)), { recursive: true });
      writeFileSync(join(tmp, path), content);
    }
    // Resolve vp from the repo's own node_modules, so plain `node scripts/gen-docs.ts` works too.
    const bins = [join(root, "node_modules/.bin"), join(root, "../../node_modules/.bin")];
    const PATH = [...bins, process.env.PATH ?? ""].join(":");
    execFileSync("vp", ["fmt", tmp], { cwd: root, stdio: "pipe", env: { ...process.env, PATH } });
    return new Map([...map.keys()].map((p) => [p, readFileSync(join(tmp, p), "utf8")]));
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

const read = (p: string): string | undefined => {
  try {
    return readFileSync(outPath(p), "utf8");
  } catch {
    return undefined;
  }
};

const map = generateFormatted();
if (process.argv.includes("--check")) {
  const changed = diffGenerated(map, read, existingDocs());
  if (changed.length > 0) {
    console.error(driftMessage(changed));
    process.exit(1);
  }
} else {
  for (const [p, c] of map) {
    mkdirSync(dirname(outPath(p)), { recursive: true });
    writeFileSync(outPath(p), c);
  }
}
