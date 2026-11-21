import { defineConfig } from "vite-plus";

export default defineConfig({
  // The lock fixture keeps the contract §6 column alignment on purpose; the conformance corpus
  // is read from splice's tests/fixtures/subset (no copy here).
  fmt: {
    ignorePatterns: [
      "tests/fixtures/lock/**",
      "tests/fixtures/explorer/**",
    ],
  },
});
