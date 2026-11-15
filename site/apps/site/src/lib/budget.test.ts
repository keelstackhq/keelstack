import { existsSync, readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";
import { expect, it } from "vite-plus/test";

// Budget from docs/agents/project.md: HTML + CSS + latin fonts for `/` stay at or under 150 KiB (153,600 B).
// Measured on the build output. Skipped locally when `dist` is not built; in CI a missing
// `dist` fails (turbo builds the site before its tests, see apps/site/turbo.json).
const dist = new URL("../../dist/", import.meta.url);
const built = existsSync(new URL("index.html", dist));
const BUDGET = 150 * 1024;
const inCI = Boolean(
  (globalThis as { process?: { env: Record<string, string | undefined> } }).process?.env.CI,
);

const read = (path: string): Uint8Array => readFileSync(new URL(path.replace(/^\//, ""), dist));
const text = (b: Uint8Array) => new TextDecoder().decode(b);
const gz = (b: Uint8Array) => gzipSync(b, { level: 9 }).length;

it.skipIf(!built && !inCI)("keeps / (HTML + CSS gzipped + latin fonts) at or under 150 KiB", () => {
  expect(built, "apps/site/dist is missing: build the site first").toBe(true);
  const html = read("/index.html");
  const css = [...text(html).matchAll(/<link[^>]+href="(\/[^"]+\.css)"/g)].map((m) => m[1]!);
  expect(css.length).toBeGreaterThan(0);
  // The browser fetches only the latin file of each family (unicode-range).
  const fonts = new Set<string>();
  for (const href of css) {
    const body = text(read(href));
    for (const m of body.matchAll(/url\((\/[^)]+-latin-(?!ext)[^)]*\.woff2)\)/g)) fonts.add(m[1]!);
  }
  expect(fonts.size).toBeGreaterThan(0);
  const total =
    gz(html) +
    css.reduce((n, h) => n + gz(read(h)), 0) +
    [...fonts].reduce((n, f) => n + read(f).length, 0);
  console.log(`budget: ${total} bytes of ${BUDGET} (${fonts.size} latin fonts)`);
  expect(total).toBeLessThanOrEqual(BUDGET);
});
