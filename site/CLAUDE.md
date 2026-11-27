# keelstackhq

Monorepo generated from hull 0.0.0-dev: pnpm workspaces, Turborepo,
TypeScript, Vite+. Packages live in `apps/*`, `packages/*`, `crates/*`. Repo-specific notes are in
`docs/agents/project.md`; per-directory notes go in `<dir>/CLAUDE.md`.

## Commands

```bash
pnpm install        # install, also runs in the SessionStart hook
pnpm check          # same as ci
pnpm run ci        # build, lint, format:check, typecheck, tests (CI job ci-site)
pnpm format         # vp fmt
```

Packages call `vp build/test/lint/fmt/pack` directly; never wrap tasks in `vp run`. Run from `site/` (`pnpm --dir site ...` at the repo root). Facts on the page come from `apps/site/src/data/facts.ts` (see `AGENTS.md`).

## Rules

- Test first: write the failing test, watch it fail, implement, watch it pass.
- Verify with real output (the `verify` skill) before saying a task is done or fixed.
- Keep files small and focused; one task per change; no drive-by refactors.
- Versions live in the catalog in `pnpm-workspace.yaml` as exact pins; use `catalog:` in packages.
- Files owned by the template (configs, workflows, `.claude/`) are updated by `splice update`;
  put local changes in seed files, `docs/agents/project.md` or `CLAUDE.md` in a package.
- Never print or commit secrets; never read `.env*`.

## Subagents

| Agent         | Model  | Use it for                                                                        |
| ------------- | ------ | --------------------------------------------------------------------------------- |
| `explorer`    | haiku  | every search, "where is X", reading many files; returns paths and a short summary |
| `implementer` | sonnet | writing code for one task, test first, then `verify`                              |
| `reviewer`    | opus   | read-only review of a diff against the task and these rules                       |
| `ci-fixer`    | sonnet | a red CI run: read the failed log, reproduce, fix                                 |

Keep subagent reports short. Skills: `verify`, `add-piece`.
`/loop` runs `.claude/loop.md`.

## Pieces

- `apps/*` (astro-site): Astro static sites with React islands and cascivo (`@astrojs/react`, `@cascivo/astro`, D-030); see `apps/<name>/CLAUDE.md`.

## Project

@docs/agents/project.md
