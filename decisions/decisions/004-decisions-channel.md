# D-004: org decisions are a splice channel

- Date: 2026-10-18
- Scope: all
- Status: accepted
- Supersedes: —

## Context

`adopt/adopt.sh` copies `DECISIONS.md` into each repo and a SessionStart hook
(`sync-decisions.sh`) refreshes it. The copy is unlocked, so drift is invisible, and the hook
needs network or `gh` access at session start. splice 0.2.0 channels give the same result with a
lock, ownership classes and `splice check`.

## Decision

Repos get the org decisions as the splice channel `gh:keelstackhq/decisions//channel`:

```bash
splice channel add gh:keelstackhq/decisions//channel --track main --piece import-project
```

Use `--piece import-claude` when the repo has no `docs/agents/project.md` (hull-generated repos
have it, so they use `import-project`). `import-agents` adds a pointer to `AGENTS.md`. The channel
owns `docs/agents/org-decisions.md` (a copy of `DECISIONS.md`, kept identical by
`scripts/check-index.sh`) and one marker section in the chosen host file. Freshness comes from
`--track main`, the splice update bot (`splice update --all`) and `splice check`.
`adopt/adopt.sh` and the SessionStart `sync-decisions.sh` hooks are retired.

## Consequences

One PR per repo: hull, splice, hoard, example-app, keelstack, berth (hoist only if the owner
asks). In each, delete `.claude/hooks/session-start.d/sync-decisions.sh` (or
`.claude/hooks/sync-decisions.sh` and its `.claude/settings.json` entry) and the hand-written
"## Org decisions" block, sync the snapshot one last time (run the old hook, or copy
`DECISIONS.md` to `docs/agents/org-decisions.md`) and commit, then run the command above: a
snapshot equal to the render adopts without `--force`, a stale one is a conflict and nothing is
written. Needs splice 0.2.0. Until a repo has migrated, its hook stays.
`adopt/adopt.sh` prints a deprecation line and is deleted once every org repo has the channel.
New repos use the channel from their first PR. Editing `DECISIONS.md` now also means
`cp DECISIONS.md channel/pieces/snapshot/files/docs/agents/org-decisions.md`.
