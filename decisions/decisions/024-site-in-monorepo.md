# D-024: The landing site lives in the monorepo and is generated from it

- Date: 2026-11-21
- Scope: all
- Status: accepted
- Supersedes: — (replaces the "site stays outside" line of D-023)

## Context

D-023 left the landing page (`keelstackhq/site`, deployed to the Cloudflare Pages project
`keelstack-landing`) outside the monorepo. Its facts (tool list, descriptions, versions, install
commands, source URLs, piece names) were typed by hand, so every release left the page behind
until someone bumped a table and re-recorded the hero. The owner's goal is that the site is
always up to date, with no excuses.

## Decision

- The site lives in `site/` (own pnpm workspace, Node 24), imported with its full history. The old
  `keelstackhq/site` repo is archived with a pointer, like the other old repos.
- Every fact the home page shows is generated at build time from this repo (`site/apps/site/src/data/facts.ts`):
  the version from `[workspace.package]` in the root `Cargo.toml`, each tool's description and
  subcommands from its crate, hull's pieces from `hull/template/pieces`, the stack pins from hull's
  base files, source URLs and links from the repository URL. A fact is never typed into the page.
  `facts.test.ts` fails when the rendered page shows a value that is not in the generated data, or
  a source file types a version or an archived repo URL.
- `site` is a folder in `.bosun/bosun.toml` that `depends_on` every tool folder it reads, so a
  change to any of them runs `ci-site` (build, lint, tests, link check) and `ci-ok` waits for it.
- `deploy-site.yml` deploys to Pages on a push to main that touches `site/` or a folder it
  depends on, and after `release.yml` finishes, from the environment `site` (main only). Pull
  requests build and test the site and upload the build; they never see the Cloudflare secrets.
- Docs pages under `/docs/` keep their pinned-release tables (`site/apps/site/src/data/versions.ts`)
  until they are moved onto the generated facts; they say which release they were verified against.

## Consequences

- A README, crate description, piece or version change reaches keelstack.dev on its merge to main,
  and the version bump reaches it again when the release finishes.
- The site's own repo-level files (workflows, bosun, splice update bot, governance) are gone: the
  monorepo's replace them. docspack (`@keelstackhq/docspack`) moves with the site; its npm release
  workflow is not ported yet.
- New secrets live in the environment `site`: `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`.
