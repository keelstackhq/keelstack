# Project notes

Notes for AI agents that are specific to this repository. `CLAUDE.md` imports this file; edit it
freely, template updates never touch it.

- What this project is: the landing page of keelstack, the family of developer tools under the
  `keelstackhq` org (hull, splice, hoard; see D-001). One static Astro site in `apps/site`,
  deployed to the Cloudflare Pages project `keelstack-landing` by `.github/workflows/deploy.yml`
  (push to `main`, or run it by hand). Nothing else lives here.
- Generated from: `keelstackhq/hull@e485214f8c7f5bebb25c31d480a9d6b6e6b45e68` with
  `node scripts/render.mjs new --template template --out <dir> --piece base --piece astro-site:site --data scope=keelstackhq --json`
  (Node 24, pnpm 12.11.2). No main `.splice/lock.toml`: only the decisions channel is under
  splice (see Update bot below).
- Commands beyond `pnpm check` and `pnpm run ci`:
  - `pnpm --filter @keelstackhq/site dev`: local dev server.
  - `pnpm --filter @keelstackhq/site build`: static output in `apps/site/dist`.
  - `pnpm --filter @keelstackhq/site preview`: serve `dist` locally.
  - Deploy: `pnpm turbo run deploy --filter=@keelstackhq/site` (Pages, branch `main`); PRs get
    `deploy:preview` (branch `pr-<n>`) from the template's `preview.yml`. Both need the
    `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` secrets (set).
- Conventions and decisions:
  - Page copy comes from the hull, splice and hoard READMEs and specs. Never invent features,
    numbers or release dates; mark gaps `TODO(copy)`. The domain is `keelstack.dev` (`apps/site/src/lib/site.ts`).
  - Use the `keelstackhq` names (D-001) even where an older README still says `keelstack`.
  - Static first: one React island, `PiecePicker` (`client:visible`) in Get started; every other
    cascivo component renders to static HTML. Fonts are self-hosted Archivo + JetBrains Mono via
    Fontsource (woff2 for latin, latin-ext and vietnamese ship in `dist`, but browsers fetch only the latin
    file via `unicode-range`; no third-party requests). Light only, no dark variant. Budget: HTML +
    CSS + fonts for `/` under ~150 KB transferred; the island JS is counted separately.
  - cascivo comes in through `@astrojs/react`, so `apps/site/astro.config.mjs` (template-owned)
    diverges from the template on purpose; keep that change when `splice update` ships.
  - Design spec: `docs/superpowers/specs/2026-10-16-landing-redesign-design.md` (supersedes
    `2026-10-16-landing-page-design.md`).
  - Update bot: `.github/workflows/splice.yml` (`keelstackhq/splice/action@v1`, `scope: all`,
    Mondays 06:17 UTC and by hand) runs `splice update --all`. With no main `.splice/lock.toml`
    that covers only the decisions channel (D-004); hull updates need a main lock first.

## Steps

1. Render hull `base` + `astro-site:site`, commit alone.
2. `pnpm install` and `pnpm run ci` green on the rendered repo.
3. Adopt org decisions (`bash ../decisions/adopt/adopt.sh .`), commit alone.
4. Project notes (this file).
5. Design spec in `docs/superpowers/specs/`.
6. Build the page in `apps/site/src/**`.
7. Verify: `pnpm run ci`, build, screenshots at 360, 390 and 1280 px (light only).
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
- 2026-10-16 Redesign: light cascivo page with the PiecePicker island, self-hosted fonts and
  `/llms.txt`; `apps/site/astro.config.mjs` now diverges from the template for `@astrojs/react`.
- 2026-10-18 Domain: `keelstack.dev` (configured on Cloudflare by the owner); canonical URL and
  `og:url` set from `apps/site/src/lib/site.ts`, except on the 404 page.
- 2026-10-22 Update bot: `splice.yml` added; `splice update --all` (splice 0.2.0) reports the
  decisions channel up to date; `splice update` alone exits 2 (no `.splice/lock.toml`).
- 2026-10-22 splice 0.2.0: Get started and the hero show the real `splice new` command, pinned to
  `@keelstackhq/splice@0.2.0` and hull `--ref v0.1.1` (`apps/site/src/lib/command.ts`); the hero
  lines are copied from that run's output (57 files: 35 owned, 8 shared, 14 seed, then
  `pnpm install`). Re-run it and update `terminal` in `content.ts` when bumping either version.
  The org repos are public (anonymous git reads of all four succeed).
- 2026-10-22 hoard 0.1.0: the hoard card shows `v0.1.0` and the README's `keelstackhq/hoard@v0`
  step (pin `@v0` while hoard is 0.x).
- 2026-10-22 hull v0.2.0: pin moved to `--ref v0.2.0`; re-ran the command (exit 0, lock hull 0.2.0
  @ `8f7eea2`): 53 files (31 owned, 8 shared, 14 seed; the `.claude/agents/*` files moved to the
  rigging channel), then `pnpm install`; the generated repo passes `pnpm check` 13/13.
- 2026-10-26 Docs 1/5: `/docs/` section (spec `docs/superpowers/specs/2026-10-26-docs-section-design.md`):
  Markdown pages in `apps/site/src/pages/docs/`, `Docs.astro` layout, page data and sources in
  `src/data/docs.ts`, Docs nav link, `/llms.txt` Docs section, Overview page.
- 2026-10-26 Docs 2/5: `/docs/getting-started/` (new or existing repo to the full setup); long
  inline code wraps in docs pages.
- 2026-10-26 Docs 3/6: `/docs/tasks/` and `/docs/ci/`; the series grew to 6 PRs with a playbook page.

## Blocked

Each item needs a human:

- **Branch protection.** Protect `main` and require the `ci-ok` status check (Settings → Branches
  or Rulesets); enable the merge queue if wanted (the workflow listens to `merge_group`).
- **Renovate.** Install the Renovate GitHub App on `keelstackhq/keelstack`; `renovate.json` is
  already present.
- **Confirm before launch.** Piece instance names other than `web` and `api` are placeholders.

<!-- splice:decisions:begin — managed by `splice update --channel decisions`; edit the source -->

## Org decisions

Cross-repo decisions from keelstackhq/decisions; follow them. To change one, open a PR there.
`splice update --channel decisions` refreshes this snapshot; commit it alone as `chore: sync org decisions`.

@docs/agents/org-decisions.md

<!-- splice:decisions:end -->
