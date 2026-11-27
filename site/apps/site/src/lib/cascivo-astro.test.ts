import { readFileSync } from "node:fs";
import { DEFAULT_LAYERS } from "@cascivo/astro";
import { expect, it } from "vite-plus/test";

// D-030: cascivo comes in through @cascivo/astro (layer order, light theme, theme script).
// Checked on the build output that src/test/build-once.ts builds once per test run.
const dist = new URL("../../dist/", import.meta.url);
const read = (path: string): string => readFileSync(new URL(path.replace(/^\//, ""), dist), "utf8");
const names = (s: string) => s.split(",").map((n) => n.trim());

it("declares the cascivo layer order first and ships the light theme once on /", () => {
  const html = read("/index.html");
  const css = [...html.matchAll(/<link[^>]+href="(\/[^"]+\.css)"/g)]
    .map((m) => read(m[1]!))
    .join("\n");
  // The minifier splits the order statement, so compare the order layers first appear in.
  const seen: string[] = [];
  for (const m of css.matchAll(/@layer ([^{;]+)[{;]/g)) {
    for (const n of names(m[1]!)) if (!seen.includes(n)) seen.push(n);
  }
  expect(seen).toEqual(names(DEFAULT_LAYERS.replace(/^@layer |;$/g, "")));
  // light.css opens its theme block with this selector; light-dark.css would differ.
  expect(css.split("[data-theme=light],:root:not([data-theme])").length - 1, "light theme").toBe(1);
  expect(html).toContain('var v="light"');
  expect(html).toContain('setAttribute("data-theme",v)');
});
