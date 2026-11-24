# site

The keelstack landing page (keelstack.dev), deployed to Cloudflare Pages `keelstack-landing` (D-024).
Own pnpm workspace (`site/pnpm-workspace.yaml`, Node 24): `apps/site` (Astro), `packages/docspack`,
`packages/shared`. Read `apps/site/CLAUDE.md` and `docs/agents/project.md` first.

## The rule: no typed facts

Every fact on the home page is generated at build time from the monorepo by
`apps/site/src/data/facts.ts`: version (the latest release tag, `RELEASED_VERSION`; else root `Cargo.toml`), tool list and descriptions (crate
`description`, hull README), subcommands (clap `Command` enum), hull pieces (`hull/template/pieces`),
stack pins (hull base files), source URLs and links (the repository URL). Do not type a version, a
`gh:` source, a tool description or a GitHub link into `.astro`/`.ts`; add it to `facts.ts`.
`src/data/facts.test.ts` fails on a typed fact, a rendered fact not in the data, a bad link, a new
tool without a card or a new hull piece the picker does not classify. Only editorial text is
written by hand (`content.ts`, `roles` in `tools.ts`, `OFFERED` in `lib/picker-data.ts`).

## Commands (from the repo root)

```sh
pnpm --dir site install --frozen-lockfile
pnpm --dir site run ci        # build, lint, format:check, typecheck, tests (CI job ci-site)
pnpm --dir site --filter @keelstackhq/site dev
```

## CI and deploy

`site` is a `[[folders]]` entry in `.bosun/bosun.toml` that depends on every tool folder, so any
change in one runs `ci-site`. `.github/workflows/deploy-site.yml` deploys on pushes to main that
touch `site/` or a dependency and after `release.yml` (`deploy-site-release.yml` dispatches it), so the site is always main's state. Version facts
(version, install commands, `@vX` sources) come from the latest `vX.Y.Z` tag reachable from the built
commit whose npm packages are all published, never from an untagged Cargo version: job `decide` lists the reachable
tags (`candidates`), the build's `Published release` step picks the first fully published one
(`apps/site/scripts/published.ts`; none = legacy pins) and sets `RELEASED_VERSION` (`loadFacts(root, released)`;
`""` = nothing released, so the legacy pins; unset = the workspace version, as in `ci-site`), then regenerates the
docs for it (`gen-docs.ts`), so the deploy is green whatever the release state (a bump push deploys the previous
facts). Jobs: `decide`, `build` (no secrets), `deploy` (secrets, only wrangler on the sha256-verified artifact).
Never deploy by hand.

## Not done yet

`/docs/**`, `/llms.txt`, `/agents.md` and the scenario blocks take their pins, sources and links
from `src/data/versions.ts`, which derives them from facts.ts. The rule: commands show only what
resolves. Until the first monorepo release (`facts.released`: the workspace version is above the move
version in `hull/legacy-tags.txt`), they pin the old repos' last releases and their `gh:` sources
(frozen history in `src/data/legacy.ts`, the only file that may type them). From that release on,
everything pins the workspace version and the monorepo sources. Links always point at the monorepo.
After the version bump, run the gen script and re-copy the bosun setup script and hull's copies
(their tests pin them to the shown versions). docspack's npm release workflow was not ported.
