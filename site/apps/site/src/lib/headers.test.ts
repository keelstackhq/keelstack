import { readFileSync } from "node:fs";
import { expect, it } from "vite-plus/test";

const headers = readFileSync(new URL("../../public/_headers", import.meta.url), "utf8");

it("caches hashed assets for a year and keeps the Markdown and assess types", () => {
  expect(headers).toMatch(/\/_astro\/\*\n {2}Cache-Control: public, max-age=31536000, immutable/);
  expect(headers).toContain("text/markdown");
  expect(headers).toContain("text/javascript");
  expect(headers).toContain("X-Content-Type-Options: nosniff");
});
