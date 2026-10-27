# docspack package

`@keelstackhq/docspack`: the site's docs as a docspack package. `build` (`scripts/build.ts`) copies
`apps/site/dist/docs/*.md` to `dist/pages/`, runs `docspack build` (name, version and `docspack.from`
come from `package.json`) and fails when a page is the source of no chunk. turbo builds
`@keelstackhq/site` first (`turbo.json`). `.llms/` and `llms.txt` are build output, not committed. The site is not a workspace dependency,
so `pnpm run ci` (`--affected`) skips this package on a docs-only change; run
`pnpm turbo run build --filter @keelstackhq/docspack` to check the pack.

- Released like a hull ts-lib (`"hull": { "release": "ts-lib" }`): bump `version` here when the docs
  change, merge, then a human pushes the tag `docspack-v<version>`; `.github/workflows/release.yml`
  (template-owned) publishes from the `release` environment with provenance.
- Never push tags or publish yourself. One-time owner setup: `docs/agents/project.md`, Blocked.
