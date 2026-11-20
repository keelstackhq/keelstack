# D-023: One monorepo, one release

- Date: 2026-11-20
- Scope: all
- Status: accepted
- Supersedes: D-003

## Context

The tools live in separate repos (bosun, splice, hull, hoist, hoard, berth, keel, decisions) that
depend on each other. Every cross-tool change needed one session, PR and review per repo, plus
relayed interface shapes. A bosun release needed pin-bump PRs in five repos, and a stale pin kept
splice main red. hull and splice kept `docs/template-format.md` byte-identical by hand. On
2026-11-20 the owner chose one monorepo, with all tools released together under one version.

## Decision

- `keelstackhq/keelstack` is the monorepo. The landing page repo is renamed to `keelstackhq/site`.
  The repo is created by the owner.
- Layout: one top-level folder per tool (`bosun/`, `splice/`, `hull/`, `hoist/`, `hoard/`,
  `berth/`, `keel/`) and `decisions/`. Rust crates form one Cargo workspace with one toolchain,
  one `deny.toml` and one pins file. Tools depend on each other by path, never by pinned
  release. hull's JS stays a pnpm/Turborepo workspace under `hull/`.
- History: each repo is imported with its full history into its folder. The old repos are
  archived with a README pointer. Archived repos stay cloneable, so old `gh:keelstackhq/<tool>`
  sources keep rendering.
- One version: every release bumps all tools to the same `X.Y.Z`. One tag `vX.Y.Z` is created,
  plus one GitHub release with every binary, and every crate and npm package is published at
  that version. Under D-022 a version-bump merge releases with no tap.
- CI: one workflow builds and tests only the affected folders (cargo and turbo), with one
  required `ci-ok` check. D-007 applies.
- bosun scopes risk rules, the trust root (D-022), leases and main health per folder, so a red
  folder blocks only PRs that touch it or depend on it.
- Agents: one root `CLAUDE.md`/`AGENTS.md` plus one per folder. `decisions/` is read in place,
  and the decisions splice channel is kept only for repos outside the monorepo.
- Outside the monorepo: the canaries (`example-app`, `cli-canary`, `zt-canary`), `site` and
  `provision`. They consume released tools as before.

## Consequences

- D-003 ends: `docs/template-format.md` is one file, at `splice/docs/template-format.md`.
- Cross-tool work is one session and one PR. Pin-bump rounds between tools disappear.
- Sources move: `gh:keelstackhq/hull//template` becomes `gh:keelstackhq/keelstack//hull/template`,
  and actions become `uses: keelstackhq/keelstack/hoard@vX`. Consumers update once.
- Secrets, the `release` environment, branch rules and Renovate are set up once, in the new repo.
- Visibility is all or nothing: all tools go public together.
