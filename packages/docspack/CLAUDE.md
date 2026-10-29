# docspack package

`@keelstackhq/docspack`: the site's docs as a docspack package. `build` (`scripts/build.ts`) copies
`apps/site/dist/docs/*.md` to `dist/pages/`, runs `docspack build` (name, version and `docspack.from`
come from `package.json`) and fails when a page is the source of no chunk. turbo builds
`@keelstackhq/site` first (`^build`). `.llms/` and `llms.txt` are build output, not committed. The site is a workspace dev dependency, so
`pnpm run ci` (`--affected`) rebuilds the pack when the docs change.

- Released like a hull ts-lib (`"hull": { "release": "ts-lib" }`): bump `version` here when the docs
  change, merge, then a human runs Actions → release → Run workflow on `main` (or pushes the tag
  `docspack-v<version>`); `.github/workflows/release.yml` (template-owned, with a local `tag` job
  that creates the tag with the release App) publishes on the tag push from the `release`
  environment with provenance.
- Never push tags or publish yourself. One-time owner setup: `docs/agents/project.md`, Blocked.
