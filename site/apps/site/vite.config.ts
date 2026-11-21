import { defineConfig } from "vite-plus";

// Test options only (lint and fmt come from the workspace root config).
export default defineConfig({
  test: { globalSetup: ["./src/test/build-once.ts"], hookTimeout: 180_000 },
});
