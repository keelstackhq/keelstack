// guard-legacy.mjs is main's guard.mjs (d90f917) verbatim except the wrapper its header documents:
// the header comment, the `fileURLToPath` import, the two renames (decide -> decideLegacy,
// failClosed -> failClosedLegacy) and everything from main's `emit` / `main()` entry to the end
// (replaced by `judgeRaw`). Main's guard.mjs is not in the tree at test time (and no git history is
// read), so this pins the sha256 of main's guard.mjs after the same normalisation (wrapper lines
// removed, renames reversed); the pin was computed from
// `git show origin/main:hull/template/pieces/base/files/.claude/hooks/guard.mjs` (full file
// sha256 5214cf6c...be43). The test applies the normalisation to guard-legacy.mjs of both template
// copies and compares the digests: any edit to the legacy rules changes the digest.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { test } from "node:test";

const PINNED_NORMALIZED_SHA256 = "3a10036e295451f4570c08d76f403cd553df24683e8eb73f9a5b21a82b4c22ac";

const normalize = (text) => {
  let t = text;
  if (t.startsWith("// Legacy judge:")) t = t.slice(t.indexOf("\n\n") + 2); // the header comment
  t = t.replace('import { fileURLToPath } from "node:url";\n', "");
  t = t.replace("export function decideLegacy(", "export function decide(");
  t = t.replace("export function failClosedLegacy(", "export function failClosed(");
  const cut = t.search(
    /^(const emit = \(decision, reason\) =>|\/\*\* main's main\(\) body verbatim)/m,
  );
  return `${(cut < 0 ? t : t.slice(0, cut)).trimEnd()}\n`;
};

for (const copy of ["template", "template-rust"])
  test(`${copy}: guard-legacy.mjs is main's guard.mjs minus the documented wrapper lines`, () => {
    const file = new URL(
      `../${copy}/pieces/base/files/.claude/hooks/guard-legacy.mjs`,
      import.meta.url,
    );
    const text = readFileSync(file, "utf8");
    assert.match(text, /^\/\/ Legacy judge: main's guard\.mjs/, "header documents the wrapper");
    assert.equal(
      createHash("sha256").update(normalize(text)).digest("hex"),
      PINNED_NORMALIZED_SHA256,
    );
  });
