# Docs section: plan

Spec: `docs/superpowers/specs/2026-10-26-docs-section-design.md`. One PR per step, each green on
its own, branch `claude/docs-<n>-<topic>` from the latest `main`. Verify each PR with
`pnpm run ci`, `pnpm --filter @keelstackhq/site build`, screenshots at 390 and 1280 px, and an opus
review of the copy against the sources.

## PR 1: layout, nav, overview

- [x] Test first (`src/data/docs.test.ts`): `docs` lists the 7 pages in order; every entry with
      `ready: true` has `src/pages/docs/<slug>.md` (index for the overview) and every page file has
      an entry; each page's frontmatter has title, description and sources in `<repo>/<path>` form
      for the eight repos; `/docs/...` links in pages point at existing pages.
- [x] `src/data/docs.ts`: `{ slug, title, summary, ready }[]` and `sourceUrl(source)`.
- [x] `src/layouts/Docs.astro`, `src/styles/docs.css`: nav, sidebar (`<details>` below 900 px),
      prose, Sources, prev/next. Only `ready` pages appear.
- [x] `astro.config.mjs`: `markdown: { syntaxHighlight: false }`; note in `apps/site/CLAUDE.md`.
- [x] Nav: "Docs" → `/docs/`; content test allows `/docs/` hrefs.
- [x] `/llms.txt`: `## Docs` with absolute URLs of ready pages (test first in `llms.test.ts`).
- [x] `src/pages/docs/index.md`: Overview.

## PR 2: getting started

- [x] `getting-started.md`, mark ready.

## PR 3: tasks and CI

- [x] `tasks.md`, `ci.md`, mark ready.

## PR 4: risk and releases

- [x] `risk.md`, `releases.md`, mark ready. Re-check bosun `feat/signed-approvals` first.

## PR 5: playbook

- [x] `playbook.md` (slug `playbook`, after `releases`), mark ready; **Practice** labels.

## PR 6: troubleshooting

- [ ] `troubleshooting.md`, mark ready; project notes progress line and the TODO(copy) list.
