# Project notes

Notes for AI agents that are specific to this repository. `CLAUDE.md` imports this file; edit it
freely, template updates never touch it.

- What this project is: the landing page of keelstack, the family of developer tools under the
  `keelstackhq` org (hull, splice, hoard; see D-001). One static Astro site in `apps/site`,
  deployed to the Cloudflare Pages project `keelstack-landing` by `.github/workflows/deploy.yml`
  (push to `main`, or run it by hand). Nothing else lives here.
- Generated from: `keelstackhq/hull@e485214f8c7f5bebb25c31d480a9d6b6e6b45e68` with
  `node scripts/render.mjs new --template template --out <dir> --piece base --piece astro-site:site --data scope=keelstackhq --json`
  (Node 24, pnpm 12.11.2). No `splice` lock yet: splice is unreleased, so `splice update` does
  not work here until it ships.
- Commands beyond `pnpm check` and `pnpm run ci`:
  - `pnpm --filter @keelstackhq/site dev`: local dev server.
  - `pnpm --filter @keelstackhq/site build`: static output in `apps/site/dist`.
  - `pnpm --filter @keelstackhq/site preview`: serve `dist` locally.
  - Deploy: `pnpm turbo run deploy --filter=@keelstackhq/site` (Pages, branch `main`); PRs get
    `deploy:preview` (branch `pr-<n>`) from the template's `preview.yml`. Both need the
    `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets (set).
- Conventions and decisions:
  - Page copy comes from the hull, splice and hoard READMEs and specs. Never invent features,
    numbers or release dates; mark gaps `TODO(copy)`. The domain is `TODO(domain)`.
  - Use the `keelstackhq` names (D-001) even where an older README still says `keelstack`.
  - Static first: no client JS except the copy-button island. Keep each page under ~100 KB
    transferred. No web fonts.
  - Design spec: `docs/superpowers/specs/2026-10-16-landing-page-design.md`.

## Steps

1. Render hull `base` + `astro-site:site`, commit alone.
2. `pnpm install` and `pnpm run ci` green on the rendered repo.
3. Adopt org decisions (`bash ../decisions/adopt/adopt.sh .`), commit alone.
4. Project notes (this file).
5. Design spec in `docs/superpowers/specs/`.
6. Build the page in `apps/site/src/**`.
7. Verify: `pnpm run ci`, build, screenshots at 390 px and 1280 px, light and dark.
8. One PR to `main`, CI (`ci` + `ci-ok`) green; a human merges.

## Progress

- 2026-10-16 Step 1: rendered from hull `e485214`; render exit 0.
- 2026-10-16 Step 2: `pnpm install` and `pnpm run ci` pass (11/11 tasks).
- 2026-10-16 Step 3: org decisions adopted (D-001..D-003).
- 2026-10-16 Step 4-5: project notes and design spec.
- 2026-10-16 Step 6: page built (hero, tool cards, fit, get started + copy island, footer, 404).
- 2026-10-16 Step 7: `pnpm run ci` 11/11; build 2 pages, about 17 KB gzipped per page;
  screenshots at 390/1280 px, light/dark, no overflow or console errors. Review nits fixed.
- 2026-10-16 Step 8: `main` created as an empty root commit by the owner; branch rebased onto it
  and opened as PR #1.
- 2026-10-16 Deploy: `deploy.yml` publishes to Cloudflare Pages `keelstack-landing`; the site's
  `deploy`/`deploy:preview` scripts use `wrangler pages deploy` instead of the template's Workers
  default (`wrangler.jsonc` is ignored by Pages).

## Blocked

Each item needs a human:

- **Domain.** Choose the domain (`TODO(domain)`), add it as a custom domain of the Pages project `keelstack-landing`
  in the Cloudflare dashboard, then add the canonical URL where `apps/site/src/layouts/Base.astro`
  says `TODO(domain)` (`astro.config.mjs` is template-owned, so `site` is not set there).
- **Branch protection.** Protect `main` and require the `ci-ok` status check (Settings → Branches
  or Rulesets); enable the merge queue if wanted (the workflow listens to `merge_group`).
- **Renovate.** Install the Renovate GitHub App on `keelstackhq/keelstack`; `renovate.json` is
  already present.
- **Public repos.** The page links to `github.com/keelstackhq/{hull,splice,hoard}`, which are
  private: visitors get a 404 until they are made public.
- **Releases.** splice and hoard are unreleased, so the page shows hull's dev renderer as the way
  to start today. Update the getting-started snippet when `@keelstackhq/splice` 0.1 ships.

## Org decisions

Cross-repo decisions from keelstackhq/decisions; follow them. To change one, open a PR there.
The hook refreshes this snapshot; commit an update alone as `chore: sync org decisions`.

@docs/agents/org-decisions.md
