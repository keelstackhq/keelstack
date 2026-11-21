# site

The keelstack landing page (keelstack.dev), deployed to Cloudflare Pages `keelstack-landing` (D-024).
Own pnpm workspace (`site/pnpm-workspace.yaml`, Node 24): `apps/site` (Astro), `packages/docspack`,
`packages/shared`. Read `apps/site/CLAUDE.md` and `docs/agents/project.md` first.

## The rule: no typed facts

Every fact on the home page is generated at build time from the monorepo by
`apps/site/src/data/facts.ts`: version (root `Cargo.toml`), tool list and descriptions (crate
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
touch `site/` or a dependency and after `release.yml`. Never deploy by hand.

## Not done yet

`/docs/**` pages still use the pinned-release table in `src/data/versions.ts` and old-repo tag
commits (`tagCommits`, `docRefs`, the bosun setup script copy). docspack's npm release workflow was
not ported.
