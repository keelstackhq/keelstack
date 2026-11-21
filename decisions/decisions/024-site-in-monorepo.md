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
  the version from the latest release tag (`RELEASED_VERSION`; without it, `[workspace.package]` in
  the root `Cargo.toml`), each tool's description and
  subcommands from its crate, hull's pieces from `hull/template/pieces`, the stack pins from hull's
  base files, source URLs and links from the repository URL. A fact is never typed into the page.
  `facts.test.ts` fails when the rendered page shows a value that is not in the generated data, or
  a source file types a version or an archived repo URL.
- `site` is a folder in `.bosun/bosun.toml` that `depends_on` every tool folder it reads, so a
  change to any of them runs `ci-site` (build, lint, tests, link check) and `ci-ok` waits for it.
- The site is always current: `deploy-site.yml` deploys main's site on every push to main that
  touches `site/` or a folder it depends on, and again when `release.yml` finishes
  (`deploy-site-release.yml` dispatches it; bosun forbids naming a `workflow_run` head or downloading
  an artifact in a workflow that runs on `workflow_run`), from the environment `site` (main only),
  always building main's head. What the page says about releases
  never comes from an untagged Cargo version: the version, install commands and sources (`@vX`)
  come from the latest release tag `vX.Y.Z` reachable from the built commit, which job `decide`
  resolves and the build reads as `RELEASED_VERSION`. A version-bump push deploys the previous
  release's facts; the release run's completion deploys the new ones. A release commit exists only
  on its tag (a child of a main commit), so a tag counts when its first parent is the built commit
  or an ancestor of it. Job `build` installs and checks without secrets and uploads one archive
  (dist and wrangler with its locked closure, `site/packages/deploy`) with its sha256; job `deploy`
  (the secrets) checks out and installs nothing: it verifies the sha256 and runs the archived
  wrangler on the archived dist. Pull requests build and test the site in `ci.yml`
  (`ci-site`) only; they never see the Cloudflare secrets.
- Docs pages under `/docs/`, `llms.txt` and the scenario pages take versions, sources and links
  from the generated facts (`site/apps/site/src/data/versions.ts` derives them). Until the first
  monorepo release (> 0.21.0) exists, commands pin the old repos' last releases, kept in the one
  frozen file `site/apps/site/src/data/legacy.ts`.

## Consequences

- A README, crate description, piece or version change reaches keelstack.dev on its merge to main,
  and the version facts follow when the release finishes.
- The site's own repo-level files (workflows, bosun, splice update bot, governance) are gone: the
  monorepo's replace them. docspack (`@keelstackhq/docspack`) moves with the site; its npm release
  workflow is not ported yet.
- New secrets live in the environment `site`: `CLOUDFLARE_API_TOKEN`, `CLOUDFLARE_ACCOUNT_ID`.
