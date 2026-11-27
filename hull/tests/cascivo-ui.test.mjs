import assert from "node:assert/strict";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import YAML from "yaml";
import { renderNew } from "../scripts/render.mjs";
import { lineCount, renderCombo } from "./helpers.mjs";

// D-030: cascivo is the default UI layer.
const read = (dir, rel) => readFile(join(dir, rel), "utf8");
const RUST = fileURLToPath(new URL("../template-rust", import.meta.url));
const UI_PIECES = ["preact-app:web", "ui-storybook", "astro-site:site"];
const uiSection = (text) => /\n## UI\n\n([\s\S]*?)\n\n## /.exec(text)?.[1];

test("root AGENTS.md has a ## UI section with each UI piece", async () => {
  for (const piece of UI_PIECES) {
    const text = await read((await renderCombo(["base", piece])).dir, "AGENTS.md");
    const ui = uiSection(text);
    assert.ok(ui, `${piece}: ## UI section`);
    assert.match(ui, /cascivo/, piece);
    assert.match(ui, /D-030/, piece);
    assert.match(ui, /https:\/\/cascivo\.com\/llms\.txt/, piece);
    assert.match(ui, /npx cascivo mcp init/, piece);
    assert.match(ui, /npx skills add cascivo\/cascivo --skill '\*' -y/, piece);
    assert.match(ui, /Tailwind/, piece);
    assert.equal(text.match(/^## UI$/gm).length, 1, `${piece}: one UI heading`);
    assert.ok(lineCount(text) <= 60, `${piece}: ${lineCount(text)} lines`);
  }
});

test("UI section sits between the pieces and Where to look, once with all UI pieces", async () => {
  const { dir } = await renderCombo(["base", ...UI_PIECES]);
  const text = await read(dir, "AGENTS.md");
  assert.match(text, /\n## Pieces\n[\s\S]*\n\n## UI\n\n[\s\S]*\n\n## Where to look\n/);
  assert.equal(text.match(/^## UI$/gm).length, 1);
  assert.ok(!/## UI/.test(await read(dir, "CLAUDE.md")));
});

test("no UI section for base-only, non-UI pieces or the rust template", async () => {
  for (const pieces of [["base"], ["base", "hono-worker:api"], ["base", "rust-cli:cli"]]) {
    const text = await read((await renderCombo(pieces)).dir, "AGENTS.md");
    assert.ok(!/## UI/.test(text), pieces.join());
    assert.ok(!/cascivo/.test(text), pieces.join());
  }
  const dir = join(await mkdtemp(join(tmpdir(), "hull-rust-")), "out");
  await renderNew({ template: RUST, out: dir, pieces: ["base", "rust-cli:cli"], vars: {} });
  assert.ok(!/## UI/.test(await read(dir, "AGENTS.md")));
});

test("Preact pieces' AGENTS.md: cascivo first, CSR only, AI links, test polyfills", async () => {
  for (const [piece, target] of [
    ["preact-app:web", "apps/web"],
    ["ui-storybook", "packages/ui"],
  ]) {
    const text = await read((await renderCombo(["base", piece])).dir, `${target}/AGENTS.md`);
    assert.match(text, /UI: cascivo first/, piece);
    assert.match(text, /`--cascivo-\*` tokens/, piece);
    assert.match(text, /packages\/ui/, piece);
    assert.match(text, /Tailwind/, piece);
    assert.match(text, /CSS-in-JS/, piece);
    assert.match(text, /no SSR/, piece);
    assert.match(text, /prerender/, piece);
    assert.match(text, /https:\/\/cascivo\.com\/llms\.txt/, piece);
    assert.match(text, /cascivo MCP/, piece);
    assert.match(text, /polyfill/i, piece);
    assert.match(text, /showPopover/, piece);
  }
});

test("catalog pins the cascivo React path for Astro at exact versions", async () => {
  const { dir } = await renderCombo(["base"]);
  const ws = YAML.parse(await read(dir, "pnpm-workspace.yaml"));
  assert.equal(ws.catalog["@cascivo/react"], "1.11.0");
  assert.equal(ws.catalog["@cascivo/astro"], "1.11.0");
  assert.equal(ws.catalog["@cascivo/themes"], "1.0.3");
  assert.equal(ws.catalog["@astrojs/react"], "7.1.9");
  assert.equal(ws.catalog["@types/react"], "19.3.0");
  assert.equal(ws.catalog["@types/react-dom"], "19.3.0");
  assert.equal(ws.catalog["@astrojs/preact"], undefined, "no piece uses it");
  assert.match(ws.catalog.preact, /^\d+\.\d+\.\d+$/, "preact-app still needs preact");
  const raw = await read(dir, "pnpm-workspace.yaml");
  assert.match(raw, /# @cascivo\/react peers on/);
});
