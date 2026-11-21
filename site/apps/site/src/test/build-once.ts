// Vitest global setup: builds the site once per test run, before any test file starts, so the
// tests that read dist/ (data/facts.test.ts, lib/budget.test.ts) share one fresh build and never
// race a rebuild. Always rebuilds: a stale dist must not make a test pass.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export default function setup(): void {
  execFileSync("pnpm", ["run", "build"], {
    cwd: fileURLToPath(new URL("../../", import.meta.url)),
    stdio: "pipe",
  });
}
