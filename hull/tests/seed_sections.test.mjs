import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const read = (rel) => readFileSync(join(here, "..", rel), "utf8");

// UC-S4-1: the seed AGENTS.md never goes stale; the lock (.splice/lock.toml) is the only version source.
for (const dir of ["template", "template-rust"]) {
  test(`${dir} AGENTS.md seed names no template version`, () => {
    const text = read(`${dir}/pieces/base/files/AGENTS.md.jinja`);
    assert.doesNotMatch(text, /template\.version/);
  });
}

test("template-rust pins.toml is owned, so update keeps the tool pins current", () => {
  assert.ok(existsSync(join(here, "..", "template-rust/pieces/base/files/pins.toml")));
  const piece = read("template-rust/pieces/base/piece.toml");
  assert.match(piece, /pins\.toml/);
  const owned = piece.split(/\n\s*seed\s*=/)[0];
  assert.match(owned, /pins\.toml/);
});
