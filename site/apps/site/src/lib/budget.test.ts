import { execFileSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";
import { beforeAll, expect, it } from "vite-plus/test";

// Budget from docs/agents/project.md: HTML + CSS + latin fonts for `/` stay at or under 150 KiB (153,600 B).
// Measured on the build output. When `dist` is missing (a plain `pnpm run test`, as bosun's test
// gate runs it), the test builds the site first with the site's own build script; turbo builds it
// before the tests in ci.yml (apps/site/turbo.json), so there `dist` already exists.
const root = new URL("../../", import.meta.url);
const dist = new URL("dist/", root);
const BUDGET = 150 * 1024;

beforeAll(() => {
  if (existsSync(new URL("index.html", dist))) return;
  execFileSync("pnpm", ["run", "build"], { cwd: fileURLToPath(root) });
}, 180_000);

const read = (path: string): Uint8Array => readFileSync(new URL(path.replace(/^\//, ""), dist));
const text = (b: Uint8Array) => new TextDecoder().decode(b);
const gz = (b: Uint8Array) => gzipSync(b, { level: 9 }).length;

it("keeps / (HTML + CSS gzipped + latin fonts) at or under 150 KiB", () => {
  expect(existsSync(new URL("index.html", dist)), "the site build produced no index.html").toBe(
    true,
  );
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
